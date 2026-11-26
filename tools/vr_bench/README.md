# vr_bench -- VR rendering benchmark and stereo regression harness

One owned game launch per run, driven through the OpenXR Simulator; the game is closed afterwards.
Details and results: `docs/vr-render-optimization-20261104.md`.

### Hand physics in an existing simulator session

Load the fixed save, stand on foot with empty hands, and leave the player facing the same scene. Enable hand collision and the FPS overlay (`xr_framegen_overlay=1`) before starting. Return the overlay to your usual setting after the session. Leave the simulator controls idle and run the driver from the terminal. It uses the loaded plugin's matching PDB for the two hand-actor flags, diagnostic counters and runtime settings layout. The regular simulator hand harness and head-pose IPC are sufficient.

```powershell
$gamePid = (Get-Process -Name Cyberpunk2077 -ErrorAction Stop).Id
py -3.13 -B tools/vr_bench/hand_physics.py --pid $gamePid --save ManualSave-491 --blocks 3
```

Use the name of the save already loaded. The save argument labels the recording. The driver records the starting player position, plugin and simulator hashes, display rate, and hashes of the port, hand-physics and game graphics settings. It attaches to that PID, changes the hand-physics master in memory, and restores the original master, simulator HMD pose (to status-file precision), and controller state on exit or Ctrl+C.

A is physics off and B is physics on. Each gets a full motion warmup before three measured ABBA blocks. Each window has a 1.5 s settling hold followed by two four-second cycles of the same HMD yaw and bilateral hand reaches at 10 Hz. The driver uses absolute deadlines, checks controller readback, observes HMD status, and records when each head command was consumed. The exercise measures unarmed reaching in the selected scene. Contact density and NPC activity remain properties of that save.

Return `build/vr-bench/<stamp>-hand-physics/hand-physics.json`. It contains both real-frame rings, their independent sample counts, display and repeat cycles, pose delivery times, SM clocks when available, and the paired physics-on minus physics-off deltas. CPU milliseconds here are **elapsed time between real frames**. The separate PhysHands counters estimate work inside the pre-step, post-fetch and finger routines. The worker resets those counters periodically, so the driver omits observed reset intervals and records their count. Inspect the individual block deltas alongside the median, especially at the display cap.

A missed pose deadline, unexpected actor state, HMD mismatch, player drift over 10 cm, or inadequate frame coverage excludes the affected block. The driver also excludes blocks whose 95th-percentile paired head-command delivery skew exceeds 25 ms. Incomplete or excluded blocks remain in the JSON. For loaded scenes, use the per-hand touched and world-contact step counts, finger contacts, calls per counted solve and near-list rebuilds to check what work actually ran.

For attribution after the first A/B, inspect `BeforeSimulate` / `PeopleStep`, the static and loose shape lists in `AfterFetch`, and the worker's `SolveFingers` under the shared near-shape lock. `MarkStacked` compares pairs when a loose list is rebuilt. The hand-collision Lua service can publish up to 96 NPC capsules in `npcPublish`. The pose changes also rebuild self-capsules in `CommitPoseInputs` and rebase their bounds in `RebasePeopleForDraw`. A lodged blade exercises additional shape-distance and weapon-box tests inside `StepBladeJoint`. The unarmed run establishes the baseline for those comparisons.

#### Storefront post on AutoSave-7

Use empty hands, hand collision and FPS metrics on, and keep the display settings fixed throughout the ABBA run. Stage the player once in the disposable session. This CET command places the feet 12 cm closer to the post than the first trial:

```lua
Game.GetTeleportationFacility():Teleport(Game.GetPlayer(), Vector4.new(-1436.76, 1259.75, 23.09, 1), EulerAngles.new(0, 0, 146))
```

Close the console, let the scene settle, then run from the workspace root:

```powershell
cmake -S tools/vr_bench/post_trace -B build/vr-bench-post -A x64
cmake --build build/vr-bench-post --config Release
$gamePid = (Get-Process -Name Cyberpunk2077 -ErrorAction Stop).Id
py -3.13 -B tools/vr_bench/hand_physics.py --case storefront-post --pid $gamePid --save AutoSave-7 --blocks 2 --cycles 2 --out build/physical-melee/post-dwell-ab01
```

The driver checks the foot position and heading and measures the native tracking target at neutral and three 10 cm controller axis offsets. A second neutral sample checks for drift. A four-second locator pass acquires a native static contact near the post. Its solved wrist, motor target, inward direction and collider bounds seed the press calibration.

Before timing, the driver tries up to five short profiles by default. Use `--post-attempts 3` for a shorter search. Each reaches, dwells with both controllers and the HMD fixed for 1.2 s, then withdraws. The return includes a small head turn. The C++ generator varies depth, lateral offset, height and wrist pitch. Each new profile uses a fresh neutral tracking target after release and four stable, advancing physics fetches. A better settled contact can seed the next probe on the same collider. The first profile that passes two separate presses and releases from matched starts is frozen for the ABBA run. Add `--calibration-only` with a fresh output folder to exit after qualification; this mode can run with the FPS overlay off.

The initial staging tolerance is 7 cm. During a probe the body can move up to 10 cm relative to its settled start while remaining in the post area. Verification and timed windows must start within 2 cm of the qualified foot. Each AB or BA pair is checked for a maximum 3 cm foot-path difference and 2° heading difference at corresponding samples. With `--post-restage-wait 30` the driver prints the qualified coordinates and allows 30 seconds for operator staging if a window starts too far away. All staging occurs before the frame rings are sampled.

Acceptance uses the original independent landmark at `(-1437.15, 1259.185, 24.8)`. After 0.4 s of settling, each enabled cycle needs three consecutive samples with positive native static-shape penetration, a solved wrist within 40 cm of the landmark, collider bounds within 15 cm, and at least 5 mm of inward distance from solved wrist to motor target. The bounds must match the live bind within 2 cm. Two clear withdrawal samples and advancing right-hand world-contact counters confirm release and contact exposure. The test checks scene identity, player transform, advancing physics fetches, and the age, center and origin of the static-shape list.

Each attempt is saved in `postCalibration.attempts`. A failed bind or loaded warmup stops the test before measurement. The windows record native steps, raw tracking-to-target clamp and target-to-wrist refusal. The first attempt, `post-ab01`, had 122 right-hand world-contact steps but only 2 mm peak inward refusal during each press. The closer station in `post-ab02` produced 225 world-contact steps, with refusal subsiding during the dwell. Both recordings supply calibration evidence only.

### Rendering benchmark commands

```
py -3.13 -B tools/vr_bench/vr_bench.py deploy                       # install the built DLL (+ its PDB) if it differs
py -3.13 -B tools/vr_bench/vr_bench.py run --label baseline          # trajectories, every frame's times, GPU per node, stereo checks
py -3.13 -B tools/vr_bench/vr_bench.py ab --label x --variant "CyberpunkVR_Foo=0" --variant "CyberpunkVR_Foo=1"
py -3.13 -B tools/vr_bench/vr_bench.py compare build/vr-bench/<A> build/vr-bench/<B>
py -3.13 -B tools/vr_bench/show_profile.py build/vr-bench/<run>
py -3.13 -B -m unittest tools/vr_bench/test_vr_bench.py            # offline tests (no game)
tools\vr_bench\build_stereo_check.bat                               # stereo_check.exe (C++)
```

- `run` options: `--traj` (see `trajectories.py`), `--stereo-on`, `--timeline-on` (GPU queue timeline: busy GPU ms per
  frame vs the frame window, and the idle time split by `tools/hitch_tests/gpu_queue_timeline.py` into starved -- the
  CPU submitted after the GPU went idle -- and held -- a fence or the driver; raw records in `timeline/<traj>.json`),
  `--stereo-sweep` (stereo checks at IPD 58/70/64 mm and a symmetric and a wide asymmetric FOV, via the simulator's live
  ipd/fov commands; the Quest 3 profile restored after), `--node-detail 0xRVA,...` (a node per pipeline), `--draw-occlusion 0xRVA,...` (an occlusion query per draw of a node: per eye and per pipeline the
  draws that left no visible sample), `--pipeline-stats 0xRVA,...` (D3D12 pipeline statistics of a node
  per eye: primitives the clipper drops, VS / PS invocations), `--census` (shader census on the still head), `--diagnostics`,
  `--setting Name=Value` (a game setting for this launch, restored after; `Name=Value#index` for a list the game
  fills at run time, e.g. `DLSS=Ultra Performance#5`), `--etw-cpu <trajectory>` (CPU of the game's process in that
  window by module, thread and port function: `etw_cpu.py`, wpr + tracerpt, elevated shell; the trace is deleted
  after the analysis), `--no-telegram`.
- Every window also reports `repro%`: the share of the XR thread's display cycles that showed the game frame the
  previous cycle had already shown (`CyberpunkVR_XrProjectionCycles` / `CyberpunkVR_XrRepeatCycles`), i.e. what the
  runtime reprojects.
- A window in which the game presented fewer than 5 frames a second is recorded under `stalls` (shown in the
  summary) and measured again, in `run` and `ab`.
- `ab`: variants of live 32-bit exports take turns ABBA in one launch, each measured on one trajectory with a GPU
  profile per node (`--stereo` adds a stereo check per variant, `--etw-cpu` wpr CPU sampling of every window by module).
  `--prime "Name=Value;..."` sets exports for `--prime-seconds` during the warm-up and restores them before the first
  window (single pass: its view-instanced pipelines are built in the background then); `--counters` reads 64-bit
  counters per window, `Name[i]` the i-th of an exported array.
- `run --census --census-modes 3,4`: every draw's vertex / pixel shader per node; the shaders seen are kept in
  `cvr_shaders/` of the run (the input of the offline shader tools below).
- Single pass (`src/Stereo/SinglePass.cpp`): `view_twin_test.exe <dxc dir> <shaders> [out]` rewrites a folder of
  shaders the way the plugin does and checks each twin validates and reads SV_ViewID; `view_pso_test.exe <dxc dir>
  <originals> <twins> [vs [ps]]` creates the view-instanced pipelines offline (`ps`: every pixel shader with a vertex
  shader it links with). Build them with `build_view_twin_test.bat` / `build_view_pso_test.bat`. The plugin loads
  `dxc\dxcompiler.dll` and `dxc\dxil.dll` beside itself; `deploy` copies them from the Windows SDK.
- Head trajectories need the simulator with `head_trajectory.h` (`simulator/`: the header and the patch of its
  `runtime.cpp`; the simulator lives in its own repository).
- Results go to `build/vr-bench/<stamp>-<label>/`; only the newest 10 stereo folders are kept, raw frames are deleted
  after the check. Telegram reports read `%LOCALAPPDATA%\vr_bench\telegram.json` (`{"token": ..., "chat_id": ...}`),
  never the repository.
- `gpu_caps.cpp`: the D3D12 feature queries this work relies on (view instancing tier, shader model, ...).
