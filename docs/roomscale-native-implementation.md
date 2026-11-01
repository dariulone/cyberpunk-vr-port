# Native roomscale implementation — 2026-09-19

## Baseline and preservation

- Working baseline: `main`, **f290ece1** (0.1.8).
- Previous experimental source, models, tests, session notes and the complete
  `render_camera_RE` tree: **backup/native-roomscale-re-20260919**, commit
  **8ca2fb40**. That snapshot is intentionally independent of this implementation.
- The native evidence reports and RE tools are available in the working tree.

## Runtime path

1. The OpenXR loop publishes an absolute, coherent horizontal head position,
   sample sequence, recenter generation, timestamp and tracking/focus validity.
2. `RoomscaleMove.cpp` intercepts **0x1402BF5A8**, the native CCT move routine
   already consuming buffered property9. It adds the unconsumed physical XY
   displacement to the game's request and calls the original once. **Original
   Z, timestep, contacts, steps, gravity and transform propagation are retained.**
3. Ownership is checked against `GetPlayer`, movement owner S+0x90, the active
   physical provider/adapter vtables and matching physics handles. Cinematic,
   vehicle, workspot, vault, slide, menu and invalid-tracking cases are gated.
4. The ledger consumes what the controller carried out of the physical step
   (the refused component along the request withheld, `AcceptedDisplacement`).
   The refused rest stays as the view's lead over the capsule: at a wall the view
   keeps the headset's pose and the body follows when the way is clear. The eye
   (camera calibration offset + tracking offset) is held within the capsule
   radius - 5 cm of the capsule's axis (0.35 m; the controller's contact offset
   adds 7 cm), where the controller guarantees free space; past that the lead is
   consumed and the view stops. `CyberpunkVR_HeadLeadsCapsule=0` restores the
   old ledger (the request consumed whole, the view held on the capsule).
5. Camera composition subtracts this span in tracking axes. MAIN, VRCAM and
   the coherent hand-anchor calculation use the same ledger. Both eyes obtain
   the shared frame's head sample even when only one won the composition gate.
6. Animation sees the full CCT velocity. Only the velocity query returning to
   **0x1406ABBB4** in LocomotionSimple's next solver removes the physical part;
   this prevents physical movement from becoming game locomotion inertia.

The interception does not set entity/world transforms or call PhysX position
setters. Scene-origin conversion remains inside the original movement path.

## Evidence used

- `cp2077-native-movement-camera-skeleton.md`: request units, P+0x80 displacement,
  property9/27, degree yaw delta P+0x9C and separate velocity fields.
- `cp2077-native-vr-off-21236.md`: linked request/consumer/CCT/T.local chain,
  owner relationships, late local/world propagation and camera binding.
- `cp2077-native-job-order.md`: physics completion, post-physics transforms,
  animation and camera order.
- `cp2077-native-locomotion-feature.md`, `cp2077-native-player-graphs.md`,
  `cp2077-native-locomotion-blend.md`, `cp2077-native-main-fk.md`: actual velocity
  and entity-local direction drive the native locomotion graph and pose.
- The Hips/driver/Spine constraint reports: body pose is downstream of graph
  evaluation; movement is not implemented by translating a skeleton buffer.
- Jump, crouch and climb reports: vertical motion and capsule dimensions have
  their own native paths; a position-setter trace is not a collision sweep.
- `render_camera_RE/analysis/roomscale_hook_abi_20260919.md` and
  `roomscale_velocity_abi_20260919.md`: exact C++ ABI and original instruction
  bytes for the two detours and the solver callsite.

The plugin checks these instruction signatures before installing the hooks.
The offline ABI check also verifies the whole EXE hash and dispatch slots.

## Heading and lifecycle

- Body-follow deadzone is **5 degrees**, with no temporal chase filter.
- Heading callback writes element **0** of its P+0x9C argument. Other indices
  target the yaw accumulator/previous delta, not the input channel.
- Translation uses `entityYaw - physicalRealign`; a physical turn does not spin
  the play-space axes. Snap/stick turning still rotates the game basis.
- Body yaw is read from the floor projection of the HMD forward vector. Near
  vertical poses hold body heading while the camera retains full pitch/roll.
- Recenter changes the sample generation and establishes a fresh baseline.
  Tracking loss, invalid values, >250ms gaps and >0.5m single-sample jumps do
  not become capsule teleports. Ordinary steps are neither eased nor rate-limited.
- Toggling movement off retains the camera correction for distance already
  consumed, so the toggle does not add the whole travelled distance again.

## Controls and diagnostics

- `bin/x64/vrport.ini`: **xr_roomscale_movement=1**.
- F10 → VRIK → **Roomscale movement**; saved by the existing settings UI.
- `xr_physical_body_rotation=1` enables the 5-degree body follower.
- `CyberpunkVR_RoomscaleHooksReady`: 1 after both detours install.
- `CyberpunkVR_RoomscaleDebug` and `CyberpunkVR_RoomscaleDebugSeq`: request,
  before/after position, resolved velocity, consumption and feedback counters.
- `[roomscale]` log once per second. `gates=F` means binding, enabled CCT,
  gameplay and tracking checks passed. Reasons: 0 applied, 1 baseline,
  2 suspended, 3 invalid tracking, 4 stale, 5 repeated sample, 6 discontinuity.

## What offline tests establish

`tools/roomscale_tests` checks:

- twelve combinations of XR/physics rates, skipped and duplicate samples;
- one-to-one camera displacement with simultaneous WASD and world scaling;
- a blocked capsule leaves the view its lead, capped at the eye limit; a stationary headset never gains
  speed into the wall, the body follows along it and closes up once it is gone; stepping away responds
  immediately;
- a native-like velocity/friction feedback loop stops after the physical step;
- free-space, opposing-input and planar wall-slide velocity attribution;
- recenter, disabled states, lost/stale/out-of-order tracking and invalid floats;
- physical 180-degree turns, snap basis, pitch-independent body heading;
- installed EXE identity, original hook bytes and the exact velocity caller.

**The contact model in these tests is synthetic.** With simultaneous native and
physical motion, separating one collision-resolved velocity into two causes is
not exact in arbitrary contacts. This build uses rejected-motion projection;
wall corners, slopes, steps and moving platforms must be checked in the game.

## Live acceptance, after the user starts the game

1. Confirm the newly loaded DLL hash, hook install lines, actual player owner,
   valid tracking and increasing CCT/feedback counters.
2. Walk physically forward/back/sideways and stop: measure request vs CCT and
   ensure no subsequent movement while the HMD is stationary.
3. Repeat with WASD/stick, at a wall, along a wall, on steps/slopes.
4. Turn physically 180 degrees, then walk forward: compare world velocity,
   body yaw, native animation direction and both eyes' camera deltas.
5. Recenter, open/close menus, lose/regain tracking, load a save and enter/exit
   a vehicle: no accumulated movement may be injected on return.
6. Check pitch, physical squat and native crouch separately. HMD height remains
   a camera/body-IK input; **capsule height is still controlled by the game's
   crouch state**. Physical crouch has not been wired to the PSM by this XY path.

No live success is claimed from a build or from these offline tests.

## Built and deployed

The first-build record below is historical. The gameplay-tier correction and
current DLL are recorded in the following section.

- Release DLL: **2,475,520 bytes**.
- SHA-256: **f2bc4feb9e8519f1d972e9c61b5443707a7c1de9b98406e8a7949b9817aa0dc5**.
- All **8** roomscale/ABI checks passed; the full native evidence verifier
  passed. Release build succeeded (existing baseline compiler warnings remain).
- Full package: `dist/CyberpunkVRPort-0.1.8-roomscale-native-20260919`.
- **256 files** deployed and re-read against their SHA-256 hashes.
  Includes all runtime CET/redscript modules, archives, shaders and both
  `vrcigarette` / `vrport` tweak directories.
- Deployment record/backups:
  `build/roomscale-deploy-20260919-101714/deployment.json`.
- Missing `vrport.ini` and `vrport-launcher.ini` restored from the verified
  pre-RE archive. Enabled roomscale and physical body rotation; `first_launch=0`.
- Game was left stopped. The user starts it before live testing.
- Implementation changes remain in the `main` working tree; only the preservation
  snapshot was committed. Nothing was pushed.

## First live run and gameplay-tier correction

PID **13464** loaded both hooks (`RoomscaleHooksReady=1`) and resolved the
player/CCT correctly. Movement remained blocked: the new code incorrectly
treated tier0 as full gameplay. The live player query and plugin memory both
reported **Tier1_FullGameplay=1**; the SDK enum defines **Undefined=0**.

The displacement and physical-yaw gates now share `IsFullGameplayTier`, using
the named SDK enum. A ninth regression check covers full, undefined and scripted
tiers. **9/9 checks pass**, and Release rebuild succeeded.

The active runtime is **OpenXR Simulator**, not Meta XR Operator. Its status
reported `VISIBLE`. The initial implementation wrongly required `FOCUSED`;
this was corrected after the background-channel check below. The simulator
command channel is `%LOCALAPPDATA%/OpenXR-Simulator/`:
`head_pose_command.json` and `controller_pose_command.json` (radians for angles).

User deployment policy: agent may kill the game for deployment; **only the user
starts it**. Iterations changing only C++ deploy **only the plugin DLL**.

- Game stopped using x64dbg; no live machine-code patches were applied.
- Only `red4ext/plugins/CyberpunkVR_Stereo/CyberpunkVR_Stereo.dll` replaced.
- New SHA-256: **654ea7bf2316d8ccce1cf6c05501d087cea690201f45b33baa7e5043508efef1**.
- Size: **2,475,520 bytes**.
- Backup/deploy report: `build/roomscale-plugin-deploy-20260919-105512/deployment.json`.
- Previous run log and DLL are retained in that directory; runtime INI hash
  was unchanged by the plugin-only deployment.
- Evidence: `docs/roomscale-live-13464-20260919.json`.
- **Movement/collision/stereo acceptance remains pending the next user launch.**

## Background simulator tracking correction (PID27052)

The next run confirmed the gameplay correction: hooks ready, native bindings
valid and gameplay allowed (`gates=F` while initially focused, then `gates=7`
in `VISIBLE`). The remaining tracking gate was unnecessarily tied to focus.

`tools/roomscale_tests/simulator_probe.py` moved the simulated head from
`(0,1.7,0)` to `(0.02,1.7,0)` and back through the atomic file-command channel.
All three status snapshots remained **VISIBLE**. No window focus or keyboard
input was used. Raw result: `build/roomscale-simulator-channel-27052.json`.
The old DLL's injected-move counter stayed0; its gate mask stayed7.

`RoomscaleTracking.hpp` now permits head tracking in **VISIBLE or FOCUSED**,
requiring both position/orientation VALID and TRACKED flags. Non-visible
session states and missing flags remain blocked. This is a general head-pose
rule, not a simulator-specific bypass. Controllers retain their existing input
handling. The tenth test covers visible/focused, inactive states and each
missing pose flag. **10/10 checks pass**, Release build succeeded.

Only the DLL was redeployed; settings unchanged, game left stopped:

- SHA-256 **ce9e3baf13f8347f236d08305ad92af700440bb7e10eecbba6f58adaa7c9c348**;
- **2,475,520 bytes**;
- backup and report: `build/roomscale-plugin-deploy-20260919-112759`.

The probe now optionally records bracketed, read-only game diagnostic snapshots
using `live_read.py`; resolve its debug/sequence addresses anew in x64dbg for
each PID. Its decoder was checked against the saved168-byte PID13464 snapshot.
Actual motion acceptance is still pending the next run of the corrected DLL.

## Positive live coverage (PID9388)

The next run confirmed basic physical XY movement, stop/no catch-up, native
entity propagation, 180-degree body follow, native animation speed/direction,
recenter of the movement ledger, a controlled wall and simultaneous W+physical
movement. See [live report](roomscale-live-9388-20260919.md) and its hashed JSON
index. No production changes/redeployment were needed during that run.

This accepts the listed basic movement cases in OpenXR Simulator, not every
terrain or render condition. The first height-transition trajectory remains
explicitly unaccepted, and rendered camera/hand continuity still needs dedicated
measurement.

## HMD-only turn regression accepted (2026-09-20)

The user reported a HMD-only camera/body offset (mouse was correct), especially
at180degrees. Eye/IPD-only corrections did not solve it. The matched mouse/HMD
trace identified the model-space eye calibration being rotated in the tracking
frame. `AnchorTranslation.hpp` now rotates the physical residual in tracking
space and rig-derived camera/eye bakes in entity/model space.

DLL `3bcdd9ee38376d40c3d0dc9abeec3a042fabdac10b2cd0383e11b7e3c5db6200`
passed14/14 offline checks, then PID19332 live validation: calibrated body-local
camera/head relation changes0.103mm over180degrees (previously235.506mm), CCT
unchanged. The user visually confirmed **«Теперь нормально»**. Full measurements:
[HMD/mouse frame report](roomscale-hmd-mouse-frame-20260920.md).

## Pending fast-translation candidate

After accepting the turn fix, the user reported body judder during physical
translation (WASD stable). Fast80cm/0.5s comparison found raw-CCT and filtered-
camera sample disagreement. A candidate now sources movement from the existing
camera frame latch, retaining raw tracking for validity/discontinuity detection.
Details and deployment status:
[translation timing report](roomscale-hmd-translation-timing-20260920.md).
The former claim about consuming one raw XR cycle is historical; the current
candidate deduplicates by camera-pose publication ID. Its live acceptance is
pending the next user launch.
