# Attach-only render/runtime audit

Prepared for goal points 6, 7, 11 and 12. This harness is **not live-validated yet**. Root owns the single game session, save protection, runtime selection, input lease, launch/exit and capture replay scheduling. It never launches/exits the game, changes active runtime, saves/loads, or opens a replay. It restores changed diagnostic exports, FG setting and simulator pose/controller state in `finally`. Failures leave the game alive for inspection.

The C++ `render_audit_math` executable generates fixed marker/pose fixtures and analyzes projection/trace data. Python/Lua only orchestrate those fixtures. The older Python dense-trajectory generators in `fgcap.py` / `capture_sweep.py` are not used.

## Fastest useful order

1. With the existing OpenXR Simulator session, take `snapshot` and fixed `markers` eye pairs. Use center, yaw ±15°, roll ±10° from the compiled fixture. The primary observation is whether each real waypoint stays on its world target in both eyes; equality of L/R pixel coordinates is not the criterion. Save raw marker traces plus both-eye images.
2. At a stationary roadside, record 10 seconds of traffic with FG off, then on. The actor trajectory/phase differs between consecutive recordings; repeat a matched scene separately if comparing quality. The raw XR delivery trace distinguishes real/generated/repeated submissions and source age. It does not measure physical display scanout.
3. On an already mounted motorcycle, make a stationary handlebar/windshield recording before moving. Repeat a short matched straight route with FG off/on, recording the windshield edges and background traffic separately. Optional `--throttle` is bounded to 0–0.4 and only applies to a verified driving-bike state; the default is no injected throttle.
4. In a separate root-controlled SteamVR null session, take baseline and per-slider snapshots. Capture actual `xrLocateViews`/`xrEndFrame` data through an already-enabled probe layer; the harness does not register one. Record the SteamVR UI values manually alongside each `--label`. Render projection and submission FOV must agree; runtime recommendations cached at swapchain setup are not proof that the current SteamVR UI setting changed them.

## Commands

Run from the repository root using an existing PID and a unique output directory:

```
python tools/render_runtime_audit/run.py --pid <pid> --runtime simulator --case snapshot --fg keep --out scratch/goal-audit-20261117/render-snapshot
python tools/render_runtime_audit/run.py --pid <pid> --runtime simulator --case markers --fg keep --out scratch/goal-audit-20261117/markers
python tools/render_runtime_audit/run.py --pid <pid> --runtime simulator --case traffic --fg off --seconds 10 --out scratch/goal-audit-20261117/traffic-off
python tools/render_runtime_audit/run.py --pid <pid> --runtime simulator --case traffic --fg on --seconds 10 --native-capture --out scratch/goal-audit-20261117/traffic-on
python tools/render_runtime_audit/run.py --pid <pid> --runtime simulator --case bike --fg off --seconds 10 --out scratch/goal-audit-20261117/bike-off
python tools/render_runtime_audit/run.py --pid <pid> --runtime steamvr-null --case snapshot --fg keep --label "outer100 inner100 upper100 lower100" --xr-trace <existing-pid-trace.jsonl> --out scratch/goal-audit-20261117/steam-fov-default
```

`--fg both` is convenient for an initial sequential check, not a matched trajectory comparison. Test each mode separately for a defensible comparison. Simulator recordings are both-eye, 60 fps, default 10 seconds. SteamVR null has no simulator compositor image or controlled yaw/roll; its marker case records native application-eye inputs at the current null pose. Those inputs do not include every compositor layer.

Timing windows preserve/temporarily enable/restore `CyberpunkVR_XrRateLog` so the
native wait-duration field is measured. Its source log is throttled to one batch
per second. Trace analysis keeps QPC completion spacing separate from predicted
XrTime spacing, excludes literal end/select placeholders in ResubmitLastFrame
rows, reports category intervals and duration correlations, and marks old all-zero
wait fields without gate metadata as unavailable/ambiguous. Completion bursts and
accepted submissions are not headset scanout or missed-deadline measurements.

Every case verifies the retained `GoalSaveGuard` token plus the engine and manager save-lock state before proceeding. Fixture mutations recheck it; throttle rechecks it every 0.5 seconds and releases input on failure. The harness never removes the root guard. `vehicle-spawn` requires an explicitly supplied, verified `--vehicle-record`; it creates one transient tagged fixture and saves its unique owner token and exact entity ID. It waits for materialization but does not mount the player. After dismounting, use `cleanup --token <saved-token>` to remove only that exact fixture ID. Empty tag results during an asynchronous spawn do not prove deletion: completion requires accepted deletion/cancellation and no managed, spawning, spawned or materialized entity. Pending cleanup retains the owner token, reports `cleanup-pending`, and fails the process. Mappins are tracked by the session's IDs and removed after their test. No existing player vehicle is deleted.

## PR 203 comparison boundary

PR head `db43c233d0a9087e196a2cf3a0d38b5845239109` contains several independent changes: synthesis/presentation queue separation, refresh feedback, producer gating, prepare-ahead, and V557 reconstruction. Parts of the queue/pacing architecture already exist in current production. The old adaptive-timer changes cannot be copied wholesale over current phase locking.

The meaningful quality comparison uses identical saved left/right native frame inputs, depth/motion, jitter, exposure and frame timing; each backend receives enough preceding frames to build its temporal history. Compare near windshield edges and distant traffic in each eye. The existing `framegen_tests` replay invokes only two generation steps and supplies a fixed 44.4 ms interval: it is a cold-history approximation, not sufficient evidence to rank V557. The existing `build/framegen-tests-203` directory name does not establish that its binary was built from PR 203. No V557 production backend is added by this harness.

## Verification so far

C++ fixture/analysis self-test passed after rebuilding the torn-trace rejection addition (1/1). Python syntax/help, standalone Lua syntax, the Lua root-lock/async-vehicle ownership fixture and three Python cleanup/status regressions passed. No new game, input, runtime switch, video or replay was run while preparing this tool. CET module behavior, marker visibility/registration, vehicle lifecycle, SteamVR null output and actual headset quality remain live checks. Before any mutating case, the root must verify the session's save lock; pending audit fixtures require cleanup or a protected reload before that lock is removed.
# Optical fixture geometry

The existing `render_audit_math optics input.json` mode is also used by the weapon
audit. It accepts measured camera position/forward/right/IPD/MAIN-eye offset,
weapon position/quaternion, authored scope offset, and an optional static ray hit.
It reports both eyes' axis distance/relief, bore/camera angle, and target depth.
Alignment is deliberately separate from visual reticle verification. Self-tests
cover a real left-eye alignment, MAIN-eye offset, eye in front of the lens,
excessive relief, sideways bore, and a close wall.
