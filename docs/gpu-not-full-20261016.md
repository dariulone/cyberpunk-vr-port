# Why the GPU is not at 100% in the city, 2026-10-16

Question: in the dense city the GPU sits at ~80-88 % instead of 100 %. Headset refresh, pacing and
locks are not involved. Scene: the Jig-Jig rooftop (CET teleport -666.89, 850.52, 64.13, yaw 178.8),
OpenXR simulator at the real render size 2648x2864, Ryzen 7 5800X (8C/16T, 15 redDispatcher workers +
GameThread), RTX 5070 Ti. All numbers are same-launch live measurements of the installed DLL; probe
scripts are in `build/monowire-housing-20261016/`.

## Short answer

The CPU frame is limited by two latency chains of almost the same length, not by CPU throughput:

| | median, ms |
|---|---|
| frame period | 15.3-15.9 |
| render chain: frame submit -> end of Present | 14.7-15.5 |
| game thread work per frame (Lua + world tick) | ~14.6 |
| GPU work per frame | ~13.0-13.7 |

The workers are idle ~64 % of the time (the semaphore count is about -10, i.e. ten workers asleep).
To make the GPU the bottleneck, **both** chains have to drop below ~13.5 ms. Shortening only one of them
helps by at most ~0.8 ms, because the other one becomes the limit.

**Status 2026-10-17:**

| step | frame period |
|---|---|
| start (2026-10-16) | 17.1 ms |
| VRCAM G-buffer partitions | 16.2 ms |
| submission chain | 14.2 ms |
| submission chain in mode 2, Lua cuts, HUD upkeep in the plugin | 13.7-14.0 ms, ~71-73 FPS |
| deferred present (Fix 3; off in Fix 4 for the pause crash, on again in Fix 5) | -0.2 to -0.33 ms, GPU +1-2 points |
| game thread leaves render jobs to the workers (Fix 3) | -0.21 ms, GPU +1.8 points |
| five async compute passes on the graphics queue (Fix 4) | -0.21 ms GPU, GPU +1.6 points |
| pause crash closed (held composition lend); every async compute pass on the graphics queue + deferred present (Fix 5) | 13.10 -> 12.73 ms, GPU 92.2 -> 95.0 % (same launch) |

GPU busy after warm-up, shipped settings (Fix 5): 12.73 ms (78.6 FPS), GPU 95.0 % in the launch with the
ABA. Two later launches with the same settings ran at ~13.75 ms / GPU ~90 %, with more GPU starvation at
the VRCAM -> MAIN hand-over: the city's CPU load differs from launch to launch by up to ~1 ms, so compare
settings only within one launch. Fully GPU-bound, this engine shows 98 % at most (see "Where it stands now"
under Fix 3).

## Fix shipped in this session

VRCAM `GBuffer_Solid` ran as one 2.15 ms job. The engine clamps RTT views (`view+0xF94 == 1`) to one
partition, and the one-byte lift (0x1D43B2B / 0x1D43B39) was only applied together with stage replay.
It now has its own setting, `xr_vrcam_gbuffer_partitions` (default 1, `CyberpunkVR_VrcamGbufferPartitionsConfigure`),
independent of `xr_stage_replay`. Same-launch ABBA: 17.10 -> 16.16 ms, 58.5 -> 61.9 FPS (+5.9 %), GPU
82 -> 85 %. A frozen-time image diff of both eyes was noise (only the animated neon sign changed).
Since 2026-10-18 the lift applies to VRCAM only (camera-name check in a trampoline at 0x1D43B2B). The
byte patch had lifted it for every RTT view with that AA mode (docs/single-pass-stage-replay-20261008.md).

## Fix 2: submission chain, 2026-10-17

The job timeline (`CVR_JOB_TIMELINE=1`) showed each view graph ending in a serial run on one or two
workers: `Synchronize` node -> Close_Graphics / Close_Compute -> Submit (ExecuteCommandLists and the
queue fences) -> the next `Synchronize` ... 2.24 ms in VRCAM and 1.90 ms in MAIN, after all recording
was already done. The cause is the job-stage builder: every submission point
(`sub_1401FAFA4(name, range, mode, builder, fences)`, called by the Synchronize node
`sub_14058DA9C` -> `sub_14058DC98` and by Submit_DrawBuffers / Submit_FrameTick /
Submit_RetirePendingResources) schedules its Close and Submit jobs into the **caller's** builder, and the
builder links its last stage to the caller's job counter. A Synchronize node therefore completes only
after its batch is on the queue, and the next one waits for that.

`src/Stereo/SubmitChain.cpp` gives every submission point a builder of its own (`sub_140218B54`):

- its first stage waits for the previous submission point's last stage (the queues get the batches in
  the original order) and for whatever the caller's builder was waiting for;
- nothing else waits for its last stage, so the next Synchronize node runs while the previous batch is
  closed and submitted;
- a job scheduled after each last stage counts the submitted points. The Present node
  (`sub_14021CDC8`) of the presenting view waits for that count before it presents. The port's
  present-time copies run inside that call. In mode 1 every view job also waits at its builder setup
  (`sub_140AA39B8`), so MAIN's graph starts after everything VRCAM submitted, as before.

Setting `xr_submit_chain` (default 1 again from 2026-11-01, with `xr_defer_present` 1: off since 2026-10-19 as
grey-materials suspects, they cut 23.85 -> 21.47 ms with a camera scope on 2.5, see
docs/scope-camera-cost-20261031.md; 0 off, 1 = view jobs and Present wait,
2 = only Present waits; live `CyberpunkVR_SubmitChain`,
counters `CyberpunkVR_SubmitChainStats[8]`). Switching it off drains the chain first.

Same-launch ABBA (3 x 4 windows of 8 s):

| mode | frame, ms | FPS | GPU busy |
|---|---|---|---|
| 0 (vanilla order) | 14.87 | 67.2 | 86.5 % |
| 1 (default) | 14.24 | 70.3 | 90.9 % |
| 2 | 14.16 | 70.6 | 91.8 % |

The Present wait is ~0.70 ms per frame in mode 1. Mode 2 is within noise of mode 1, and mode 1 keeps
the original VRCAM -> MAIN order, so mode 1 is the default. Frozen-time image check of both eyes: pixels
differing by more than 20 levels, mode vs mode 0, were at or below the 0-vs-0 noise (left 237-315 vs
407, right 2445-4591 vs 5245). No timeouts in any run (`stats[6] == 0`).

With the render chain shortened, the game thread is now the longer of the two loops.

Repeated in the configuration a player has (QuickBoot's launch-tool sampling off; it costs 0.03 ms) after
the Lua batch below: mode 0 14.97 ms / 66.8 FPS / GPU 87 %, mode 1 14.19 / 70.4 / 90.9 %, mode 2
13.79 / 72.5 / 92.8 %, mode 2 better than mode 1 in all four windows. A 240 s soak in mode 2 with a head
yaw sweep and live 1 <-> 2 switches had no timeouts and no slow windows. **Mode 2 is the default since
then.**

## The game thread after the render fixes, 2026-10-17

Chain timeline in mode 2 (`CVR_CHAIN_TIMELINE=1`, medians):

| | ms |
|---|---|
| frame period | 13.67 |
| render chain: frame submit -> end of Present | 13.01 |
| game thread waiting for the render scene | 0.00 |
| game frame `sub_140292A54` | 11.28 (world tick 10.47 of it) |
| game thread between two game frames | 2.07 |

The game thread never waits for the render any more; it is the limit. The 2.07 ms between game frames
is CET's `onUpdate` of all mods (the per-mod meter sums to 2.08 ms). It runs serially before the world
tick, so each 0.1 ms of Lua is 0.1 ms of frame time until the GPU becomes the limit (~12.8 ms of GPU
work per frame here, 0.8-0.9 ms away).

### Lua per frame

Per-mod meter (`luameter.py`, os.clock and collectgarbage("count") around each callback), same scene:

| | before | after batch 2 | after the HUD upkeep |
|---|---|---|---|
| all mods, ms/frame | 2.245 | 2.083 | see below |
| mods' garbage, KB/frame (without Stereo) | 22.3 | 15.4 | 15.5 |
| Stereo (GC step + its modules) | 0.761 | 0.703 | |
| TacticalBelt | 0.348 / 7.3 KB | 0.257 / 3.2 KB | |
| VRIK | 0.162 / 2.6 KB | 0.117 / 1.2 KB | |
| ForceFPP | 0.087 / 2.5 KB | 0.083 / 1.5 KB | |
| Weapon | 0.103 / 1.9 KB | 0.099 / 1.5 KB | |
| CETBridge (debug tool, unchanged) | 0.283 | 0.287 | |

What changed:

- **TacticalBelt.** The belt walk re-read `IsEnabled()` on all 21 pieces and the worn piece's
  `chunkMask` every frame (0.13 ms). It now keeps what it last read or set: three pieces re-read per
  frame in rotation, the mask every eighth frame, `IsDefined` still on every piece every frame
  (`performance.legacyBeltWalk` = the old walk; same-launch A/B 0.13 -> 0.04 ms). The player's entity
  hash is computed once a frame instead of twice, and the pause check, `applyElements` (4 Hz, ~60
  components) and the highlight scan (20 Hz) no longer build a closure per call (`pcall(fn, args)`
  instead of `pcall(function() ... end)`), nor a substring per component.
- **ForceFPP, Weapon.** The same closure removal on the per-frame paths; the natives they call are
  unchanged (VRRemoteCamera / VRBraindance / VRSceneCamera(0) still go out every frame, because their
  "off" paths release state).
- **VRIK.** The locomotion blackboard through `GetPlayerStateMachineBlackboard()` with the field id read
  once; the FPP camera component once a frame instead of twice; the second `Game.GetPlayer()` gone.
- **hud_panel.** See the next section.

Every changed file was compile-checked with LuaJIT 2.1 (`tools/hitch_tests/luacheck.py`) and its set of
global reads/writes compared with the previous version (`tools/hitch_tests/luaglobals.py`): a local
that moved below its user, or a function lifted out of a closure, shows up there as a new global.

### HUD root upkeep in the plugin

`modules/hud_panel.lua` cost 0.30 ms a frame (live section meter, `hudlive.py`):

- 0.15 ms: the per-frame loop over the roots it found (6 roots, 3 visible). For each visible root it made
  ~10-18 CET calls: visibility, sizes, the crop `inkHudEntryInfo`, window size, two
  `FlagForVisualInvalidation`, and for the interaction hub the `topWidgets/looting` test.
- 0.11 ms: `VRHudPanelUpdate` itself.
- The once-a-second controller scan: ~1.5 ms each time.

The scan stays in Lua unchanged. It hands the roots over through `VRHudPanelRootsBegin` /
`VRHudPanelRoot` / `VRHudPanelSubtitle`, and the per-frame part runs in C++ at the start of
`VRHudPanelUpdate` (`src/Natives/HudPanel.cpp`, `CyberpunkVR_HudRootsNative`, counters
`CyberpunkVR_HudRootsStats`):

- `FlagForVisualInvalidation` is `sub_1402EFFE0` (its script native was checked in IDA);
- the child position and size use the engine functions the slot geometry already calls (0x894E50 /
  0x894DFC);
- the window size goes through the widget's own `SetSize` via RTTI.

A root the plugin cannot take makes the script keep its own upkeep.

Gotcha: `GetUserData` gives the script an `inkUserData` reference. A native parameter declared
`handle:inkHudEntryInfo` arrives as null from CET, so the parameter is `handle:inkUserData`, type-checked
in C++. The first build had this and silently did no upkeep: 0 roots in the counters.

Same-launch check:

- Counters: 6 roots and 3 visible per frame, as the script had. No crop or window writes in the
  steady state.
- Frozen-time image of the composited view, plugin vs script: below the same-path noise.
- Lua meter ABAB: 1.84 / 1.89 ms (plugin) vs 2.07 / 2.06 ms (script).
- `VRHudPanelUpdate` cost: 0.097 -> 0.102 ms.

### Where it stands

All of the above installed, same-launch ABBA of the HUD upkeep (plugin vs script): 14.04 vs 13.95 ms,
GPU 92 %. The 0.2 ms of Lua does not show in the frame time. With the game thread at ~13.1 ms, the
render chain at ~13.0 ms and ~12.9 ms of GPU work per frame, all three loops are within a few tenths of
a millisecond. Cutting one of them alone hands the limit to the next, and per-frame variance of the two
CPU loops leaves the GPU idle ~6-8 %. Reaching ~100 % needs both CPU loops comfortably below the GPU:
~1 ms off the game thread **and** off the render chain. What is left to try:

- Render chain: let a submission point's Close jobs run while the previous batch is still being
  submitted, and keep only Submit -> Submit in order (today the whole next point waits; the Present
  wait is ~0.7 ms).
- Game thread: the HUD scan (1 Hz, ~1.5 ms spike), `VRHudPanelUpdate` (0.10 ms), the TacticalBelt
  highlight scan (0.065 ms), the GC step (~0.25 ms, proportional to the remaining ~18 KB/frame of
  garbage). CETBridge, the debug bridge, costs 0.29 ms a frame when installed.

### Ruled out on the way

- **Render job priority.** Render node jobs (`RunRenderJob` 0x985AC4, `RenderNode` 0xA98398) run from
  dispatcher queue 1. Queues 3..0 are served highest first, and the world tick's builder uses priority 2.
  Moving render jobs to queue 0 or 2 changed nothing (14.66 / 14.92 / 14.93 ms) and was removed.
- **Render jobs delaying the end of the world tick.** The game thread's own lag after its last job
  was 0.001 ms.
- **FlushCounter's other dequeue mode** (`sub_14014B0B4`, all queues, blocking) brought no gain.

## Fix 3: where the GPU waits, and the deferred present, 2026-10-17

### GPU queue timeline

`src/Render/GpuQueueTimeline.cpp` (asked for through `CyberpunkVR_GpuTimelineRequest`, nothing installed
otherwise) adds two pre-recorded command lists to each direct / compute `ExecuteCommandLists` call, each
writing a GPU timestamp, before and after the game's lists. With the CPU time of every call, the queues'
clock calibration and the Present marks, `tools/hitch_tests/gpu_queue_timeline.py` splits every stretch
where no queue runs anything into:

- **starved**: the batch that ended it reached the driver after the GPU had already gone idle;
- **held**: it was submitted earlier but started later (a fence on the other queue, or the driver).

`tools/hitch_tests/gpu_chain_overlay.py` puts the same capture on the chain timeline (game started with
`CVR_CHAIN_TIMELINE=1`).

City, before the change below (~30 direct and 6 compute submissions per frame):

| per frame | ms |
|---|---|
| GPU idle | 2.1 |
| starved, almost all 4.5-7 ms after Present | 1.3 |
| held, ~30 small gaps | 0.84 |

What the overlay showed:

1. The game thread's scene wait ends 0.06 ms after the Present hook returns. Only then does the frame
   submit start.
2. The next frame's first real VRCAM batch reaches the GPU ~5.5 ms later (frame tick, VRCAM culling and
   occlusion).
3. At the end of Present the GPU has only ~4-5 ms of MAIN's work queued, so it runs dry for ~1.5 ms.

The Present node `sub_14021CDC8` itself took 1.3-2.0 ms at the very end of the chain: ~0.55 ms waiting for
the last batches to be submitted, then the DXGI Present with the port's present-time work (0.69 ms hook).

### Deferred present

The node calls three device setters with the output handle (+56/+104/+144), then the XeSS-FG present id
(`sub_141D577B0`). Then comes `sub_14021C5AC`:

- the DXGI Present;
- a queue Signal of the present fence;
- the back-buffer index rotation;
- `sub_14021C820`, which waits for the next back buffer's fence and sets the back buffer the next frame
  draws into.

After that it refreshes the window and clears a per-frame device flag (`device+18008` → `+498`).

`SubmitChain.cpp` now (`CyberpunkVR_SubmitChainDeferPresent`, default 1) lets the presenting node:

1. take the output object (+3976) and handle (+416) from the render data;
2. clear the flag;
3. queue the rest as an engine job (`sub_14014299C`) after the chain's last submission point, with a
   counter of its own (`sub_140142B20`) that becomes the chain's tail.

Ordering guarantees:

- The next frame's batches reach the queue after the present and after the port's present-time copies.
- Every view job of the next frame waits for it at its builder setup (`sub_140AA39B8`). Only the frame
  submit, frame tick and proxy flush overlap it.
- `ResizeBuffers` waits for it.
- At most one present is in flight.

At first only the presenting view's job waited. VRCAM's nodes then ran beside the port's present-time
work (HUD capture, render-target bookkeeping). In a soak of 120 s of head turns followed by pause / unpause,
that crashed twice:

1. a stale render-data pointer in a node, with `hk_OMSetRenderTargets` → `hud_adopt_by_node` on the stack;
2. an engine assert, descriptor heap full.

The same soak with the session's changes switched off survived. With every view gated, every soak of the
shipped build passed (sweep, three pauses, a teleport across the city). The VRCAM job waits in ~27 % of frames, ~0.8 ms
when it does.

It is used only in the steady state: a second graph (VRCAM) ran the node in this or the previous frame,
and the output object has been the same for 600 presents. The handle changes every frame. At start-up,
deferring the very first presents crashed inside the NVIDIA driver while CET built its D3D12 overlay in
its Present hook.

Results:

- Live ~20k deferred presents per run with no stall.
- Frozen-time image of both eyes: same as without (the only differences are an animated sign, the same
  in both modes).
- GPU timeline: starved time 0.81 → 0.22 ms per frame; the game thread no longer waits for the chain.
- Same-launch ABBA, only the presenting view gated: 14.24 → 13.91 ms (GPU 89.7 → 91.9 %) and 14.42 → 14.20
  ms (88.4 → 89.5 %).
- Every view gated, ABBA at the end of each soak: 14.50 → 13.84 ms (GPU 87.6 → 91.7 %) and 14.22 → 13.67 ms
  (90.7 → 93.4 %).

The world tick then lengthens by ~0.5 ms, because the next frame's render now overlaps it more on the
CPU. The game thread (Lua ~1.9 ms + world tick 10.1-10.7 ms) sets the period, and it equals the GPU's work
including the gaps between submissions (~13.3 ms).

### The game thread leaves render jobs to the workers

Stack samples of the game thread with the deferred present on (thread_stack_sampler, 250 Hz):

| share of game thread time | % |
|---|---|
| world tick | 77 |
| CET / Lua | 15 |
| spinning in FlushCounter (QPC, yield, PeekMessage) | ~50 |
| render node jobs (`RunRenderJob` 0x985AC4 / `Detour_NodeDispatch`) | 6.8 |

The 6.8 % in render node jobs is ~0.9 ms a frame. The game thread waits through `sub_1402933F0` →
FlushCounter `sub_14014AB30(dispatcher, counter, 1, -1, …)`, whose dequeue `sub_14014AF14` (used nowhere
else) takes jobs from dispatcher queues 3, 2 and 1. Render nodes are queued on 1, the world tick's jobs on
2. A render node taken just before the world tick's last job finishes keeps the world tick open.

`src/Stereo/GameThreadQueues.cpp` replaces that dequeue while `CyberpunkVR_GameThreadSkipRenderQueue` is 1
(the default; live switch, stats in `CyberpunkVR_GameThreadQueueStats`):

- it tries queues 3 and 2 only;
- a job on 1 stays for the fifteen workers;
- if nothing is found, the job token goes back exactly as in the original's empty path.

This differs from the earlier "return render jobs" test, which took them and put them back (~7200 requeues a
frame).

Same-launch ABBA, 16 windows: 14.17 → 13.96 ms (+1.7 % FPS), GPU 89.2 → 91.0 %. Seven of the eight "on"
windows are faster than every "off" window.

### Async compute

Of the ~1.1 ms of held gaps that remain, ~0.86 ms are 5-6 cross-queue waits per frame of 50-500 µs, at
direct ↔ compute transitions, while neither queue runs anything. Async compute overlaps only ~0.2 ms of
work (compute busy 0.85 ms per frame).

The CET switch `GameOptions.SetBool('Rendering/AsyncCompute', 'Enable', false)` moved everything to the
direct queue:

- 30 → 18 direct submissions per frame;
- held gaps 0.87 → 0.36 ms;
- ABBA 13.97 → 13.42 ms, GPU 90.9 → 92.9 %.

But one eye went black. Pass by pass:

- `ExposureScaleBuffer` on the graphics queue stops the port's XR loop (the exposure twins expect it on
  the compute queue).
- `BuildDepthChain`, `DPL`, `DynamicTexture`, `ExtendedDuringSSR` and `FlattenNormals` keep both eyes at
  the noise level.

Switching passes live a dozen times ran the engine out of descriptor heap space (assert in
`gpuApiDX12Descriptors.cpp:424`). The A/B was therefore done with the five safe passes set to false in the
installed `vrcam_cpu_tweaks.ini` from start-up, then switched back once and forth once after a 150 s warm-up:

| | frame | FPS | GPU | compute submissions/frame | held gaps |
|---|---|---|---|---|---|
| five passes on the graphics queue | 13.40 ms | 74.7 | 95.8 % | 4 | 0.68-0.94 ms |
| original | 13.61 ms | 73.5 | 94.2 % | 6 | 1.12 ms |

The original block was slower than both blocks around it. Even so, the change is **not shipped**. In
soaks (120 s of head turns, then pause / unpause, then a teleport), builds with these five on the graphics
queue crashed twice in three runs when pausing:

- a stale render-data pointer in a node (`exe+0x95b4ac`);
- an engine assert, descriptor heap full.

The final build with every pass async passed every soak. `mods/config/vrcam_cpu_tweaks.ini` keeps all
passes async, with this written next to them.

`SSAO` on the graphics queue keeps the image, but the frame stays the same (13.27 vs 13.26 ms); it stays
async. `LutGeneration` on the graphics queue changes one eye completely, so it stays async.

### Where it stands now

The shipped build (deferred present with every view gated, game thread off the render queue, all
async compute passes async), after a 180 s warm-up:

| | value |
|---|---|
| frame | 13.33-13.40 ms (74.9 FPS) |
| GPU busy, nvidia-smi | 95.1-95.8 % (median 95.2) |
| Fix 3 on vs off, same-launch ABBA, 12 windows | 13.57 → 13.38 ms, GPU 93.1 → 94.6 % |
| starved, GPU timeline | ~0.1-0.25 ms per frame |
| held gaps between ~30 submissions | ~1 ms per frame |

Soaks of this build (120 s of head turns, three pauses, a teleport across the city): three passed, plus a
seven-minute measurement run; none crashed.

**The ceiling.** The same scene with the port's render size raised to 3456x3744 (`vrport-launcher.ini`,
restored afterwards) is fully GPU-bound:

- GPU work 20.6 ms a frame, frame 22.4 ms;
- starved time 0;
- nvidia-smi 98.0 % flat in every window.

That is the most this engine reaches. The ~1 ms of gaps between its queue submissions a frame stays even
with an endless queue. At the headset's size the remaining distance is ~3 points: the game thread (Lua
~2 ms + world tick ~10 ms) is level with the GPU's work plus those gaps (both ~13.3 ms).

**Warm-up.** The first ~3 minutes in the city run ~0.7 ms slower (streaming, shaders). After that the frame
settles at 13.3-13.4 ms with GPU 94.5-96 %. All A/Bs here interleave their states, so the warm-up does not
bias them, but absolute numbers taken early are high.

## Fix 4: what the idle GPU time is made of, and the pause crash, 2026-10-17

### Where the idle time goes

The GPU queue timeline now also names the code that submits on each queue and records every queue
`Signal` / `Wait` (kinds 3 and 4 in `CyberpunkVR_GpuTimelineRecords`, `load_sync` in
`tools/hitch_tests/gpu_queue_timeline.py`). Hardware GPU scheduling is on (checked with
`D3DKMTQueryAdapterInfo`, WDDM 2.7 caps).

| idle per frame, city | ms |
|---|---|
| direct <-> compute hand-overs, ~75 us each (40-55 us at best) even when the signal was queued early | ~0.5 |
| OpenXR simulator's compositor queue (`openxr_simulator.dll`), ~150 us per hand-over, two per frame | ~0.13 |
| ~25 submission boundaries on the direct queue, ~8 us each | ~0.2 |

Every submission also waits on a CPU-signalled fence first (residency-style); that costs nothing measurable.
The simulator's queue belongs to the simulator; a headset runtime has its own.

On the CPU side:

- stack samples: the game thread spends ~60 % of its time spinning in FlushCounter while the world tick's
  jobs run;
- the workers are busy 42 % of the time; the port's own code is ~6 % of their busy samples;
- pinning the game thread to a core of its own, with the workers kept off its SMT sibling: no gain.

### Async compute on the graphics queue

All async compute passes on the graphics queue, from start-up (one live switch for the comparison):

| | frame | FPS | GPU | held gaps |
|---|---|---|---|---|
| every pass on the graphics queue | 12.70 ms | 78.8 | 96.9 % | 0.34 ms |
| all async | 13.27 ms | 75.3 | 93.9 % | 1.04 ms |

One eye went black with `LutGeneration` on the graphics queue. The cause was the port's grading lend
(`CyberpunkVR_LutSrcLend`), not the engine: with the lend off that eye matched the async picture. The lend
replaced every descriptor table that `GenerateTonemappingLUT` binds with MAIN's handle from the previous
frame. On the graphics queue those tables are fresh copies in the shader-visible heap, and among them are
the tables the build writes, so the second view built into MAIN's. `Stereo/CommandListCensus.cpp` now:

- notes the descriptor copies made inside the node;
- keeps MAIN's sources at its bind;
- copies them into the second view's own table, only for the eight-texture source table;
- leaves the other tables (14, 22, 19, 24 descriptors) as the engine made them;
- keeps the old handle lend for a table whose fill it did not see.

Counters: `CyberpunkVR_LutLendByContent`, `CyberpunkVR_LutLendTables`. Both eyes now match the async
picture, by day and by night at street lights.

### The pause crash

Pause stress: head turns, then pause / unpause, 25 times (`probe_pausestress.py`). The crash was in MAIN's
UI composition node `sub_14020A264` ("renderstage_composition_element"), through `sub_141D57280` ->
`sub_1401F1208`, in the clear `sub_14095B35C`. Its resource table entry pointed at a texture object that had
been freed, its memory already holding UI vertex data. The full stack came from an x64 unwinder written for
the crash dumps (scratchpad `unwind.py`); the scanned stack of the crash report is full of stale frames.

| setting | result |
|---|---|
| all async, no deferred present | 25 pauses + a soak, clean |
| five passes on graphics, no deferred present | 25 pauses + a soak, clean |
| every pass on graphics, no deferred present | crash at the 3rd pause |
| five passes on graphics + deferred present | crash in 3 of 4 runs |
| all async + deferred present, only the presenting view gated | crash |
| all async + deferred present, every view gated | 3 soaks clean |

It is a timing race. Both the deferred present and moving more work to the graphics queue make it more
likely. Gating render packets on the deferred present (`CyberpunkVR_DeferredPresentPacketWaits`) never
fired: no packet ran beside a present. What was found but not proven: `sub_14021C5AC` counts the present
and signals the present fence after the DXGI Present.

**The port's HUD upkeep is part of it.** Every pass on the graphics queue, no deferred present:

| HUD | result |
|---|---|
| as shipped | crash at the 3rd pause |
| VR HUD panel off (`xr_hud_panel=0`), root upkeep on | crash at the 11th pause |
| `HudPanel.tick` off once in the world | 25 pauses clean |
| upkeep repaints run at the frame submit | 25 pauses clean, then a second run crashed at the 22nd |

The upkeep repaints the HUD roots and their separate windows every frame (`FlagForVisualInvalidation`). That
happens in CET's `onUpdate`, at the start of the game thread's frame, while the previous frame's render can
still be in the composition node. `Natives/HudPanel.cpp` now:

- queues those repaints and runs them on the game thread at the frame submit `sub_14029384C`, where the
  previous render has finished and the next has not started;
- runs them in the next update when no frame submit came in between (menus, loading).

Live switch: `CyberpunkVR_HudRepaintAtSubmit`; counters: `CyberpunkVR_HudRepaintStats`. The second crash was
earlier in the same node (element registration `sub_1401ECBE0`): the composition list it walks (render data
+360, 464-byte elements) lives in per-frame UI memory that had been reused. With the deferred present, the
moved repaints still crashed at the 9th pause. The cause was found later: Fix 5.

What shipped at this point (superseded by Fix 5):

- the deferred present **off** by default;
- the five passes that passed the stress run on the graphics queue;
- `ExposureScaleBuffer`, `LutGeneration`, `SSAO` and `RaytraceASBuild` async;
- the HUD upkeep's repaints at the frame submit (still in).

## Fix 5: the pause crash closed, every pass on the graphics queue, 2026-10-17

### The cause

The crashing node was the **second view's** DrawComposition, not MAIN's. The port lends the second view a
composition object (`xr_comp_lend_set`, `Stereo/ViewReuse.cpp`): that is what gives the second eye its HUD
layers and the braindance outline. The lend read the UI set's live handle (holder at set+176, pair at +600)
on every dispatch and wrote it into the view's render data +0x168 **without a count of its own**. The set
belongs to the UI and moves on while the previous frame's render is still running:

- the engine assigns MAIN's handle on the game thread, from the same set: in the world from the view fill
  `sub_14029CDD8` (return 0x29CEB1), in menus and loading from the game frame `sub_140292A54` after the frame
  submit (`sub_14029AA08`, return 0x29AA53);
- after that the game thread draws the next frame's UI;
- with a long world tick that drawing came after the second view's graph had finished; in a pause the tick
  is short and it overlaps, so the second view walked a list that was being rebuilt or freed.

That explains every earlier observation: faster render settings, the deferred present and the HUD upkeep's
per-frame repaints all made the overlap more likely, and the render-packet gate never fired because no
packet ran beside a present.

Pause stress (25 pauses after head turns, then a teleport), every async compute pass on the graphics queue:

| setting | result |
|---|---|
| composition lend off (`xr_comp_lend_set=0`, diagnostic only: one eye loses its HUD) | 25 pauses clean |
| lend on, as before | crash at the 3rd pause (Fix 4) |
| lend on, object held | 25 pauses clean |
| lend on, object held, deferred present on | 25 pauses clean (37 866 deferred presents) |

### The fix

`Detour_CompAssign` (the hook on the assign `sub_140201A68`) now copies the handle MAIN received from the
lent set, with a count, through the engine's own handle copy `sub_140ACE86C`, into two slots: this assign's
and the previous one's. An object stays alive for two more assigns after the second view last read it. The
lend writes the held object instead of reading the set. Live switch: `CyberpunkVR_CompLendFrameHold`
(0 = read the set as before). Counters: `CyberpunkVR_CompLendFrameStats` = [0] assigns held, [1] lends from
the held object, [2] of them where the set's live object was a different one, [3] holds from the game frame,
[4] holds from the view fill, [5] assigns of another set skipped.

In the world the held object is the set's live one at almost every lend (3 differences in 28 463 lends
without the deferred present, 332 in 49 264 with it): the second eye sees the same HUD as before, only with
the object's lifetime guaranteed. Both eyes against the old lend in the same launch: 0.5-0.8 % of pixels
differ by more than 24 levels (the same as two shots of the same setting). The first version held only the
game-frame assign; in the world that froze a menu object and one eye's HUD layer became a black rectangle.
The picture check caught it.

### What shipped

- every async compute pass on the graphics queue (`mods/config/vrcam_cpu_tweaks.ini`, `Enable = true`, all
  keys `false`);
- the deferred present **on** again (`CyberpunkVR_SubmitChainDeferPresent = 1`);
- the held composition lend.

Same launch, 180 s warm-up, ABA (new, old, new), against the settings shipped in Fix 4:

| | frame | FPS | GPU (nvidia-smi) |
|---|---|---|---|
| every pass on graphics + deferred present | 12.73 ms | 78.6 | 95.0 % |
| five passes on graphics, no deferred present | 13.10 ms | 76.3 | 92.2 % |

GPU queue timeline: held gaps 0.71 -> 0.32-0.35 ms per frame, submissions 25 -> 17 per frame. Both eyes
have the same mean brightness in all three states.

### Tried after Fix 5, no effect (reverted)

- `GameOptions.SetBool('Rendering/AsyncCompute', 'Enable', false)` live: the compute queue kept its two
  submissions per frame, and the launch stayed ~1 ms slower after the switch (both states). Do not
  live-toggle `Enable`.
- `[WaterSimulation] UseAsyncComputeFFT = false` in the ini, from start-up: still two compute submissions
  per frame, 13.77 vs 13.76 ms. The config table (`{key, group}` pointers at 0x1432ffca8) has it default
  true; `Rendering/AsyncCompute` has exactly eleven keys (`Enable` + the ten in the ini).

What is left on the compute queue: two submissions per frame from the engine's generic submit
(`exe+0x1F5DDF`), one 0.12-0.18 ms at 5.5-9.5 ms after Present and one of ~3-5 us; the compute queue
waits on the direct queue four times per frame, the direct queue on compute twice. Not identified.

### Where the rest goes: a run at 81.5 % (2026-10-17)

A normal session on the same rooftop (8 m from the test spot, 02:03 game time, free head movement),
OpenXR simulator 1.5.0 at 2648x2864, read live from the running game (`CyberpunkVR_PresentTiming` and one
GPU queue timeline capture):

| | this run | best test launch |
|---|---|---|
| frame | 15.3 ms (~64 FPS) | 12.7 ms |
| GPU work per frame | 12.46 ms | ~12.3-12.5 ms |
| GPU starved (no work submitted) | **2.5 ms** per frame | 0.4-0.7 ms |
| GPU held (waiting for another queue) | 0.32 ms | 0.33 ms |
| nvidia-smi | 77-83 % | 95 % |

- Same GPU work and the same held gaps: what Fix 3-5 removed stays removed.
- No pacing in the frame: the Present hook spends 0.3 ms in DXGI Present, nothing in frame-generation
  pacing or the XR pump; no other process uses the CPU.
- The starvation sits at **6.5-11 ms after Present**: the hand-over from VRCAM to MAIN. MAIN's view job
  starts only after VRCAM's last node, and its CPU preparation (culling, software occlusion, collection,
  ~3 ms; see "The render chain") runs while the GPU has finished VRCAM's work and has nothing of MAIN yet.
- How long that preparation takes depends on the scene: direction of view, night lights, crowd and
  traffic. The tests looked in one fixed direction. The test launches themselves ranged 12.7-13.75 ms
  with the same settings.

So the GPU percentage depends on the scene, and below ~95 % the limit is this CPU hand-over, not the GPU
or the queues.

What would remove it (not done):

1. Prepare MAIN while VRCAM renders (see "What would be needed for 100 %" and the failed view overlap
   attempt). The old attempt died in `DeclareCommonResourceAllocs`; that function also reads the
   composition field (+0x168) the port lent without a count, so it may be worth retrying now that the
   lend holds its object. Not verified.
2. Make MAIN's CPU preparation cheaper, e.g. reuse VRCAM's culling for MAIN, without losing objects at the
   edges of MAIN's view.
3. Stage replay (`xr_stage_replay`) removes ~1.5 ms of job CPU but is off in the installed `vrport.ini` and
   has known defects (stages 10 and 24 excluded).

Read-only tools used for this (in the session scratchpad, simple to rebuild): the live split of the Present
hook from `CyberpunkVR_PresentTiming` ([1] period, [2] finish + pacing, [3] XR pump, [4] DXGI Present,
[5] whole hook), and one `CyberpunkVR_GpuTimelineRequest` capture analysed with
`tools/hitch_tests/gpu_queue_timeline.py`.

## The render chain

Engine structure (2.31 build):

- The game thread runs the game frame `sub_140292A54`: the world tick, then at 0x292C33 it waits on the
  render scene's handle `scene+8` (vfunc +0x310 = `sub_1402933E4` -> `sub_1402933F0` -> FlushCounter
  `sub_14014AB30`), then submits the frame (vfunc +0x10 `sub_14029384C`: RenderFrameTickResetPools,
  proxy flush, RenderFrameTick) and calls FlushRenderScene (vfunc +0x18 `sub_140293AF4`) for **MAIN
  only**.
- FlushRenderScene builds a staged job group chained on `scene+8`: proxy flush (`sub_14039D2FC`) ->
  sub-scenes (`sub_140293E34` -> IssueRenderSubScene `sub_140167EB4` / `sub_140167FC8`, which issues the
  **VRCAM** view job) -> DispatchDrawBuffersFlush (`sub_140AC3FE0`) -> the MAIN view job
  (`sub_140293568` -> `sub_140AA3904` -> `sub_140219730`).
- VRCAM is an RTT sub-scene. Its view job's completion includes its whole graph's node jobs, so **MAIN's
  view job starts only after VRCAM's last node** (measured: MAIN job at +7.46 ms, VRCAM last node at +7.50).

Per cycle, from the frame submit (medians):

| step | ms |
|---|---|
| submit -> VRCAM view job (proxy flush: `FlushParallel_Safe/NotSafe...`, `RemoveAndAddPendingProxiesToScenes`, RenderFrameTick) | 0.9 |
| VRCAM prepare: `RunRenderNodeJobs` 0.35, `FlowAllocator_*` 0.3, software occlusion `Vis/*` 0.6 (4 rasterization passes), `DoCull_*` + `MainSceneDrawables_TestTargetGroups` + `_Collect` 0.6 | ~2.2 |
| VRCAM geometry burst + tail (only `Synchronize` nodes, ~4 of 15 workers busy) | ~3.7 |
| VRCAM end -> MAIN view job (stage hops) | 0.5 |
| MAIN prepare (same pipeline as VRCAM) | ~2.9 |
| MAIN burst + tail | ~4.1 |
| Present | 0.65 |

During the prepare windows only ~3 (VRCAM) to ~6 (MAIN) workers are busy: the stages are already
parallel, and what costs time is the dependencies between them, not the work.

## The game thread

- Lua (CET) ~2.5-3 ms before the world tick: the Stereo GC step ~0.7, TacticalBelt ~0.4, CETBridge ~0.2
  (debug tool), the rest small.
- World tick `sub_14026C044` ~11.0 ms. The game thread spends ~10 ms of it in FlushCounter, and ~60 % of
  that is spinning (QPC / yield / PeekMessage) with nothing to run. The job timeline shows the vanilla job
  graph: three entity buckets (PreTick -> ServiceEvents -> transforms -> physics -> animation -> PostTick),
  animation ~2-6.4 ms, `wwiseEventPerform` up to 5 ms per job, UI ticks and streaming ~7.3-9.2 ms, and at
  the end a **serial** run of HUD virtual windows (`UI/VirtualWindow/TickLayout -> Arrange -> WidgetDraw ->
  DrawElements -> UI/FullScreenLayer/Draw`, one window after another, ~1 ms, little CPU). These windows
  are the game's own `inkHUDLayer` windows; the port only reads them.
- The world tick gets ~1-1.5 ms shorter when the render is cheaper (looking at the sky), but its end is not
  tied to any render event (all differences have sd >= 1.4 ms, while its own duration has sd 0.6-0.85 ms).

## What would be needed for 100 %

After Fix 5 the queues are no longer the issue; what is left below ~95 % is item 1, measured in
"Where the rest goes: a run at 81.5 %".

1. **Render chain -2 ms:** run MAIN's prepare (culling, occlusion, collect) during VRCAM's geometry
   burst, with MAIN's recording and submission still after VRCAM. This means changing the engine's
   scheduling of the sub-scene versus the main view. Risks: the shared transient allocator
   (`FlowAllocator_*`), command-list submission order across the two graphs, and the port's own
   VRCAM -> MAIN dependencies (shared cascades, wide merge SSR/SSPR/AO, viewData). Gating MAIN nodes by
   blocking workers can deadlock, because the burst releases up to ~50 nodes at once.
2. **Game thread -1 ms:** trim the port's Lua (GC step, TacticalBelt and similar).

Expected result if both work: the frame becomes GPU-bound at ~13.5 ms (~+13-15 % FPS).

### View overlap attempt (failed, code removed)

Tried under `CVR_VIEW_OVERLAP=1`, with a live switch for same-launch A/B:

- MAIN's view job (`sub_140293568`) took as its prerequisite the proxy-flush stage's completion,
  captured after `sub_14039D2FC` from `builder+16`.
- A signal job (`sub_14014299C`) took its old place in the final stage, so `scene+8` still waited for
  VRCAM.
- MAIN render-node jobs were to be "parked" until the signal: not run and their counter left untouched.
  The worker-loop patch cleared `rbx` to skip the counter. They would then be put back with
  `sub_1401436F4` plus a semaphore permit.

Results:

1. With `Selection` / `PrepareCollector` / `DoCulling` allowed early, the game crashed instantly in
   `exe+0x1FA56B`, inside `DeclareCommonResourceAllocs` (`0x76ECC4`). With only `DoCulling` allowed, it
   crashed the same way. The two views' graphs share per-frame render state that the engine only touches
   serially.
2. A view cannot be identified from a node job before it runs. Mode 2 compared each job with the view
   `NodeDispatch` actually saw:
   - `data[0]` matched the MAIN view object for only ~0.5 % of nodes of either view;
   - the job's completion counter never matched the view job's (each node has its own).
   
   With an empty allowlist, the one job that got parked was in fact VRCAM's. The signal never came and
   the game hung.

Conclusion: overlapping VRCAM and MAIN needs per-view isolation of the engine's per-frame render state
plus graph membership of every node. That means re-architecting render-graph execution, not a hook.

## Ruled out (measured, no gain)

- **Worker idle spin** (dispatcher `qword_143438980` +0x500, read by every idle worker before it sleeps on
  the semaphore at +0x4C8). It is already 10240. Live A/B: 4000 ≈ 10240 ≈ 20000; 100000 is worse
  (16.88 ms).
- **Game thread running render node jobs while it waits** (FlushCounter runs jobs through
  `sub_14014B164`). Returning `sub_140985AC4` jobs to the queue, the way FlushCounter does, gave
  16.18 vs 16.20 ms. The game thread re-dequeued ~7200 jobs per frame. Removed.
- Lua burn A/B (slope 0.2), XR waits, GPU fences, worker count, and port overhead alone (Present tail
  ~0.74 ms: dxgi 0.22, OnPresent 0.37, desktop mirror 0.10; ECL hooks ~0.22 ms against 1.46 ms of driver
  time).
- The busy unnamed thread (~12 % of a core) is the port's OpenXR frame/submit thread (`xrWaitFrame`,
  simulator preview), unrelated to the world tick.

## Tools added

- `tools/hitch_tests/node_timeline.py --pid P --seconds S --out spans.json` / `--analyze spans.json`:
  the node timeline (`CyberpunkVR_NodeTimeline`, `src/Stereo/Profiler.cpp`, ring now 1<<19 spans), plus:
  - **chain steps** (kinds 6-15: scene wait, frame submit, view flush, view job, game frame,
    RenderFrameTick, sub-scene, world tick, game-thread counter waits) when the game is started with
    `CVR_CHAIN_TIMELINE=1`;
  - **every job with its profiler name** (kinds 20 worker / 21 game thread) with `CVR_JOB_TIMELINE=1`
    (the worker-loop call at exe+0x143244 is patched to go through `JobRun`; ~3600 jobs per frame).
  
  Both live in `src/Stereo/ViewChainTimeline.cpp` and are not installed in a normal run. The environment
  variables pass through `launch_to_save.py`.
- `tools/hitch_tests/timeline_jobs.py`: hitch_profiler worker samples (`perfT0`) overlaid on the node spans.
- Present / ExecuteCommandLists section timers: `CyberpunkVR_PresentSections[8]`, `CyberpunkVR_EclTiming[8]`.
- Probes: `probe_timeline.py`, `probe_jobs.py`, `probe_jobtl.py` (+ `run_jobtl.ps1`), `probe_spin.py`,
  `probe_sky.py`, `probe_mthelp.py`, `probe_sections.py`, `probe_partitions.py`.
- GPU queue timeline: `src/Render/GpuQueueTimeline.cpp` (request through `CyberpunkVR_GpuTimelineRequest`,
  records in `CyberpunkVR_GpuTimelineRecords`), `tools/hitch_tests/gpu_queue_timeline.py` (idle split into
  starved / held per frame and queue) and `tools/hitch_tests/gpu_chain_overlay.py` (the same against the
  chain timeline).
- `build/monowire-housing-20261016/stereometer.py`: section meter of the Stereo mod's onUpdate, like
  `beltmeter.py`. Probes for Fix 3: `probe_gpuq*.py`, `probe_defer.py`, `probe_gtq.py`, `probe_asyncini.py`,
  `probe_async2.py`, `probe_gpubound.py`, `probe_soak.py`, each with its `run_*.ps1`.
