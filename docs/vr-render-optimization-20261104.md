# VR rendering optimisation under engine stereo (from 2026-11-04)

Working log of the frame-time work on the default stereo path (`xr_engine_stereo=1`: the 2.5 Odyssey 3D side-by-side
renderer driven as the headset's two eyes, `engine-stereo-plan.md`). Every number here comes from
`tools/vr_bench` runs on this machine; nothing is extrapolated.

**This machine** (`tools/vr_bench/gpu_caps.cpp`, D3D12 feature queries): NVIDIA GeForce RTX 3060 12 GB,
ViewInstancingTier 3, Shader Model 6.8, MeshShaderTier 1, enhanced barriers, wave ops (32 lanes), resource binding
tier 3. NOT the RTX 5070 Ti of the earlier docs (another PC): their FPS figures do not compare with these.
Game settings: DLSS Transformer, Balanced; the per-eye render size the engine actually uses is 1197x1294 (DLSS input,
upscaled to the eye's 2648x2864); SSR and AO off, ray tracing off, volumetric fog Medium. OpenXR Simulator, Quest 3
profile, 90 Hz. Save ManualSave-491 (Megabuilding H10 street, raining). Foveated rendering in any form (fixed / eye-tracked / quad views, foveated DLSS,
VRS foveation) is excluded: `vr_bench run` refuses to start with `xr_foveated_dlss` or `xr_vrs` on.


## Result (final benchmark with every optimisation, `build/vr-bench/20261105-150104-final3-estereo`)

Same save, trajectories and settings as the baseline, one launch each (`vr_bench compare`), single pass and the
streaming placement on (their defaults):

| trajectory | GPU p50 ms | GPU p99 ms | CPU p50 ms | FPS |
|---|---|---|---|---|
| still | 23.28 -> 22.15 | 24.63 -> 24.43 | 23.22 -> 22.00 | 43.0 -> 45.2 |
| yaw_slow | 23.47 -> 22.01 | 24.68 -> 24.07 | 23.42 -> 21.87 | 42.7 -> 45.5 |
| yaw_fast | 23.21 -> 21.62 | 24.75 -> 24.04 | 23.14 -> 21.60 | 43.2 -> 46.1 |
| snap90 | 22.49 -> 21.24 | 52.73 -> 22.99 | 22.53 -> 21.31 | 36.3 -> 47.0 |
| combo3 | 23.52 -> 22.16 | 25.69 -> 23.77 | 23.49 -> 22.19 | 42.4 -> 45.0 |
| translate_lr | 22.94 -> 22.34 | 24.22 -> 24.97 | 22.88 -> 22.22 | 43.6 -> 44.6 |
| lean_in | 23.40 -> 22.24 | 24.50 -> 23.48 | 23.42 -> 22.22 | 42.7 -> 44.9 |

- GPU p50 -3 to -7 % on every trajectory, FPS +2 to +7 % (snap90's p99 see below). Against the previous final run
  (before single pass and the streaming placement): GPU p50 -1 to -4 %, FPS +1 to +4 %. Launch-to-launch noise is
  about +-1 %; each optimisation's own same-launch A/B is in its section.
- Stereo (L->R / R->L agreement, mean difference): still 0.939 / 1.76, yaw_slow 0.949 / 1.59, yaw_fast 0.977-0.957 /
  1.44, translate_lr 0.937-0.943 / 1.41 -- the same range as before the changes.
- CPU (ETW, yaw_fast): 188.4 ms a frame summed over cores; the port 11.7 (single-pass draw handling the largest,
  1.1 ms), NVIDIA driver 15.8.

Earlier final benchmark (`build/vr-bench/20261105-001535-final2-estereo`, before single pass and the streaming
placement), against the same baseline:

| trajectory | GPU p50 ms | GPU p99 ms | CPU p50 ms | FPS |
|---|---|---|---|---|
| still | 23.28 -> 22.82 | 24.63 -> 25.22 | 23.22 -> 22.67 | 43.0 -> 43.9 |
| yaw_slow | 23.47 -> 22.83 | 24.68 -> 25.36 | 23.42 -> 22.69 | 42.7 -> 43.8 |
| yaw_fast | 23.21 -> 22.53 | 24.75 -> 24.17 | 23.14 -> 22.49 | 43.2 -> 44.4 |
| snap90 | 22.49 -> 21.49 | 52.73 -> 23.63 | 22.53 -> 21.63 | 36.3 -> 46.3 |
| combo3 | 23.52 -> 23.11 | 25.69 -> 24.98 | 23.49 -> 23.03 | 42.4 -> 43.4 |
| translate_lr | 22.94 -> 22.72 | 24.22 -> 24.99 | 22.88 -> 22.60 | 43.6 -> 44.1 |
| lean_in | 23.40 -> 22.82 | 24.50 -> 23.89 | 23.42 -> 22.79 | 42.7 -> 43.9 |

- The optimisations, each measured in its own ABBA A/B (below): the XR submit no longer re-copies an already submitted
  frame (-0.6 ms GPU p50); the tiled light pass lights both eyes in one dispatch (RenderLightBuffers -0.25 to
  -0.31 ms); the draw hook skips work for switched-off features (~0.35 ms of CPU a frame, off the GPU-bound frame
  time). Between the two launches: GPU p50 -0.2 to -0.7 ms, FPS +1 to +3 % on every trajectory but snap90.
- snap90's p99 52.7 -> 23.6 is NOT a result of this work: the baseline's spike never came back in later launches
  (see Head-turn hitches); it is listed because it is what the two runs measured.
- Stereo, moving head included: L->R / R->L geometric agreement 0.937-0.954, mean difference 1.29-1.97/255, inner
  edge 1.29-2.35 (before the changes, same build without them: 0.939 / 1.76-1.80; the baseline launch's own stereo
  captures failed, fixed since).
- Node table: ApplyDLSS 6.5 -> 0.3 and VolumetricFog 3.3 -> 0.5 ms are the profiler's attribution, not savings (the
  DLSS evaluates are now pseudo-nodes of their own, and ExecuteIndirect ranges are marked).

Single-pass stereo (added after the final benchmark above, section "Single-pass stereo with D3D12 view instancing"):
93 % of the G-buffer draw pairs are drawn as one view-instanced draw for both eyes; GPU p50 22.80 -> 22.38 ms, the
NVIDIA driver's CPU 22.9 -> 14.7 ms a frame (summed over threads), same-launch ABBA x3, pictures unchanged; on by
default (`CyberpunkVR_SinglePass`).

What was examined and why it stayed as it is (each measured, details below): per-eye object culling (both eyes send the
same 1.90 M primitives, culled once for the union of the lens windows; only objects wholly in the other eye's strip
could go, and D3D12 sees no bounds), SSR and AO (per pixel, no work repeated between the eyes; halving the cone AO
dispatch saved 3.2 ms but broke the image), the DLSS right-eye copies (DLSS neither writes nor reads correctly at an
offset), the Present node's work (it is the back buffer the eyes are copied from), CPU scheduling (GPU busy 98 % of
a frame, no starved idle), CPU work (below 14 ms; the frame is GPU-bound down to there). The frame is ~6 ms of DLSS
(two Transformer evaluates at 2648x2864), ~9 ms that does not shrink with the render resolution in all, and per-pixel
scene work that stereo cannot share without changing what each eye sees.

## What the existing stereo path is (audit, before any change)

From the IDA and capture work already in `docs/` (`engine-stereo-plan.md`, `sbs-shader-audit-20261027.md`,
`engine-stereo-reflections-20261102.md`, `cpu-afw-20261104.md`) -- the facts the work below starts from:

- **Draw submission: two re-sends, not one instanced draw.** The engine traverses the scene once (one culling with
  the centre camera over the union of both eye windows, shared shadow cascades), then every per-eye pass re-sends its
  draw batches once per eye with that eye's half viewport and camera block (b1 CameraShaderConsts, 852 B; b1 row 53
  = the eye, 1 left / 2 right). Whether any engine pipeline is created with D3D12 view instancing is checked at run
  time below (the 2.5 shader corpus is not on this machine; the earlier single-pass audit found no ViewID in the
  shaders it converted).
- **Targets: one side-by-side texture per G-buffer / depth / colour resource**, left eye x in [0, W/2), right eye
  [W/2, W). Not arrays. `_RightView` targets of some passes are half width.
- **Per eye:** depth prepass, G-buffer, decals, deferred lighting (tile culling per eye), SSR traces (dispatch per
  eye), contact shadows, transparents, particles, distortion, UI; DLSS per eye (the port's split,
  `EngineStereoDlss.cpp`). **Once for the frame:** scene traversal, cascades and local shadow maps, decoupled
  particle lighting (texture space), volumetric clouds (one dispatch over both halves), cone AO (one dispatch over
  the side-by-side frame, two-halves mode), auto exposure.
- **Shader stereo state:** b0 GlobalShaderConsts describes the whole side-by-side frame (row 32.x two-halves flag);
  b1 the eye; b12 SharedPixelConsts both eyes' matrices (rows 81-84 / 101-104 inverse, 93-96 / 113-116
  world->clip). The port's DXIL twins (`tools/asym_tests/*`) already correct the screen-space passes that mixed the
  two (fog light tiles, cone AO cross-eye steps, SSR / contact shadow inner-edge continuation, view direction from the
  eye instead of the stereo centre, particle allocator, light-cull bounds, clouds' fog lookup).

## Tools (this work)

- `tools/vr_bench/vr_bench.py run|compare|deploy`: one owned launch per run; recorded head trajectories in the
  simulator (`head_trajectory.h`, evaluated at each frame's display time); every frame's CPU interval and GPU time
  (plugin `CyberpunkVR_FrameRing*`); GPU time per render-graph node per frame with the eye of each draw range
  (`CyberpunkVR_SceneGpuFrameRequest` + `CyberpunkVR_SceneGpuViewportDetail`); nvidia-smi utilisation / VRAM;
  per-thread CPU; stereo captures checked by `stereo_check.exe` (one eye reprojected into the other by depth:
  geometric agreement, colour difference, inner-edge band, heat maps). Raw frames are deleted after the check, the
  newest 10 stereo folders are kept. Results in `build/vr-bench/<stamp>-<label>/`, summaries to Telegram.
- `tools/vr_bench/test_vr_bench.py`: offline tests (trajectories, statistics, eye split, a synthetic stereo pair for
  `stereo_check.exe`'s camera and depth conventions).

## Measurements

### Baseline (build/vr-bench/20261104-210511-baseline-estereo)

Every frame of each trajectory (CPU = interval between real frames at the Present, GPU = the game queue's span from
the frame's first submission to its Present):

| trajectory | FPS | CPU p50/p95/p99 ms | GPU p50/p95/p99 ms | 1% / 0.1% low |
|---|---|---|---|---|
| still | 43.0 | 23.22 / 23.95 / 24.14 | 23.28 / 24.21 / 24.63 | 41.1 / 41.1 |
| yaw_slow (47 deg/s) | 42.7 | 23.42 / 24.25 / 24.84 | 23.47 / 24.36 / 24.68 | 39.8 / 39.5 |
| yaw_fast (283 deg/s) | 43.2 | 23.14 / 24.22 / 24.73 | 23.21 / 24.28 / 24.75 | 40.0 / 40.0 |
| snap90 (386 deg/s) | 36.3 | 22.53 / 47.08 / 53.47 | 22.49 / 46.94 / 52.73 | 6.6 / 4.1 |
| combo3 | 42.4 | 23.49 / 24.71 / 25.51 | 23.52 / 24.87 / 25.69 | 38.4 / 37.6 |
| translate_lr | 43.6 | 22.88 / 23.82 / 24.27 | 22.94 / 23.94 / 24.22 | 40.2 / 39.7 |
| lean_in | 42.7 | 23.42 / 24.22 / 24.53 | 23.40 / 24.35 / 24.50 | 40.3 / 40.3 |

GPU-bound (nvidia-smi 99 %). The snap90 spikes did not repeat in the next launch (diag run: snap90 p99 25.0 ms as
the first turn of that launch); its GPU queue timeline showed every long frame fully busy (window 28.05 ms / busy
27.51 ms), i.e. GPU work, not the CPU starving the GPU. Open: reproduce with repeated snaps after the world has
streamed the turned-to view once.

Stereo agreement (stereo_check, one eye reprojected into the other by depth): geometric agreement 0.94-0.95, mean
colour difference 1.3-1.7 of 255, p95 4-5, > 32: 0.1-0.3 % of the compared texels, inner edge 1.1-2.3.

### GPU per render-graph node, still head (ms per frame, 12 profiled frames)

| node | ms | eye split |
|---|---|---|
| ApplyDLSS (per-eye evaluate + the right eye's copies) | 6.3-6.6 | |
| RenderElements (depth, G-buffer, decals...) | 3.9 | L 2.2 / R 1.7 |
| RenderLightBuffers (tiled deferred lights, ExecuteIndirect) | 2.6 | |
| RenderLightsIntegrate | 1.8 | L 0.9 / R 0.9 |
| DecoupledParticleLighting (async compute) | 0.6 | |
| VolumetricFog | 0.5 | |
| Bloom+tonemap 0.5, ScreenSpaceRain 0.45, Present 0.41, HUD 0.31, CAS 0.30, GI 0.28, shadow mask 0.30 | | |

Profiler correction found on the way: ExecuteIndirect was not a marked command, so its GPU time ran on in the range of
the command before it. The first profile put 3.3 ms on VolumetricFog; with ExecuteIndirect marked, VolumetricFog is
0.5 ms and the 2.6 ms belong to RenderLightBuffers.

### Same-launch A/B of suspected costs (ABBA x2, still)

| variant | GPU p50 ms |
|---|---|
| base | 23.05 |
| CyberpunkVR_RuntimeDiagnostics=1 | 23.00 |
| fog twins off (CyberpunkVR_AsymFogPatch=0) | 23.02 |
| one DLSS evaluate over both eyes (CyberpunkVR_EngineStereoDlssPerEye=0; measurement only, it smears) | 22.57 |

The per-eye DLSS split costs ~0.5 ms over one evaluate (second evaluate + the right eye's input/output copies).
Launch drift within one launch is up to 0.8 ms (weather, NPCs), hence ABBA.

### The tiled light pass

Census: every tiled-light permutation is an ExecuteIndirect issued twice a frame (once per eye). The heaviest
(`1d8f7c21d7457039`, 1.34 ms for both) walks the tile list of the WHOLE side-by-side frame in each eye's dispatch;
a pixel of the other eye returns after one tile-list load and one depth load (b1 row 53 vs the half the pixel is
in). Wasted work per dispatch is small next to a lit pixel; not pursued.

### Single-pass ceiling (ABBA x2, `CyberpunkVR_GeometryCostProbe` with the new eye bits)

| variant | RenderElements right eye | GPU p50 |
|---|---|---|
| base | 1.687 ms | 23.44 |
| right-eye draws with an empty scissor (IA/VS/setup only) | 0.743 ms | 20.57 |
| right-eye draws skipped | 0.080 ms | 19.67 |

(The frame-level drops include cheaper lighting over an empty right G-buffer; they are not single-pass gains.)

D3D12 view instancing can take at most the right eye's IA/VS/setup: **0.74 ms GPU**, and only if the driver runs the
view-independent part of a vertex shader once (tier 3 here); plus the CPU recording of ~2,200 right-eye draws a
frame. One pipeline carries 1,587 draws per eye and 0.74 ms per eye of RenderElements.

### More same-launch A/Bs (ABBA x2, still)

| variant | GPU p50 ms |
|---|---|
| base | 23.39 |
| every stereo correction twin off (fog, SSR, AO, shadow, merge/eye-view, light cull, distant lights, probe cull, particle facing / lighting, bloom, exposure, screen fx) | 23.16 |
| DLSS left eye only (`CyberpunkVR_EngineStereoDlssMode=4`, diagnostic: the right eye is not upscaled) | 19.33 |

All the port's DXIL corrections together cost ~0.23 ms. The right eye's DLSS evaluate with its copies costs ~4.1 ms:
DLSS Transformer at 2648x2864 out of 1197x1294 is ~3+ ms an eye on this GPU, the largest single cost of the frame.

### Draw calls: where they are, and how many leave nothing on screen

Reference: the Fallout 4 mod "Draw Calls Reducer" (nexusmods 110754, its description and ini): there most draws are
sun shadow cascades redrawn every frame, so it caches cascades and local light shadows (static/dynamic split), draws
static casters GPU-driven, culls small casters and culls world geometry with a Hi-Z snapshot of earlier frames
(moving objects, screen edges and camera jumps protected). The same questions asked of this renderer:

Commands per frame by render-graph node (scene profile, final run): RenderElements (both eyes' G-buffer) 10,697 of
11,853 = 90 %; sun shadow cascades 315, rain map 208, local shadow maps 156, distant shadows 78 -- shadows are ~5 % of
the draws here, and their GPU time is small (cascades 0.04 ms, local 0.22 ms at Low shadow resolution).

An occlusion query on every draw (`CyberpunkVR_SceneGpuDrawOcclusion`, `vr_bench run --draw-occlusion <node>`; a draw
whose query counts 0 samples put nothing on screen: hidden, outside the eye's view, sub-pixel or discarded whole),
H10 street, still head (`build/vr-bench/20261105-101415-draw-occlusion`, `*-draw-occlusion-pso`):

| pass | draws / frame | no visible sample | 1-64 samples |
|---|---|---|---|
| RenderElements, left eye | 4,508-5,225 | 49.5-50.6 % | 19.4-20.1 % |
| RenderElements, right eye | 4,508-5,225 | 51.5-51.6 % | 19.7-19.8 % |
| sun shadow cascades | 232 | 43.6 % | 1.5 % |
| local light shadow maps | 138 | 30.8 % | 41.7 % |

- Half of the G-buffer draws of each eye leave no sample. They are spread over the pipelines (129 in the pass; the
  main material pipeline, 3,174 draws a frame for both eyes, 46 % empty; several smaller ones 78-96 % empty): the
  engine's visibility lets through much geometry that is hidden or too small, it is not one pass gone wrong.
- What culling them would be worth, from the measurements above: the right eye's whole IA/VS/setup costs 0.74 ms of
  GPU, so its empty half is in the order of 0.3-0.4 ms an eye; on the CPU ~2.3 k draws an eye at ~0.45 us of driver
  time each (the right-eye measurement below) is ~2 ms of driver CPU a frame for both eyes, plus the engine's
  preparation of those draws.
- Why it is not built at the hook: a culling decision needs the object's bounds before its draw (the mod's Hi-Z test
  has them from the engine); the D3D12 boundary only sees draws, and only afterwards whether they were empty. Using
  last frame's per-draw result (predication) would not be conservative -- an object that comes into view would appear
  a frame late (pop-in), which this work does not accept -- and would save only GPU time. Using one eye's result for
  the other is not conservative either (parallax). The place for it is the engine's visibility system.
- Shadows: the mod's cascade cache and small-caster cull address ~5 % of this renderer's draws; the local shadow maps'
  41.7 % tiny casters (1-64 samples) are the closest match to its small-caster cull.

### GPU occlusion culling prototype (`CyberpunkVR_OcclusionCull`, off by default, `src/Stereo/SinglePass.cpp`)

Built to measure what culling at the D3D12 boundary can give, not to ship. Each single-pass (view-instanced) G-buffer
draw gets a binary occlusion query; ONE query covers both views of the draw, so its result is conservative across
the eyes (an object either eye sees counts). A draw is known from frame to frame by its pipeline, index and vertex
buffers, arguments (not the per-frame instance offset) and its occurrence in the frame; its results are resolved at
the command list's Close into a buffer kept in PREDICATION state, and in mode 1 a draw found empty the frames before is
predicated away (`SetPredication`, EQUAL_ZERO on its last result). Every 8th frame nothing is predicated so culled
draws are tested again -- a draw coming into view can be up to 8 frames late (pop-in): the reason this cannot ship
under the goal's no-pop-in rule. Mode 2 runs the queries without culling (their own cost).

First version, still head, ABBA x2 (`build/vr-bench/*-ab-occ-proto`): ~2,400 queried draws a frame, 1,600-3,500
distinct draws known. Query cost dominated everything: GPU p50 22.48 -> 57.7-76.5 ms with the queries alone, 71.4-72.4
ms culling. The cause: each list's draws had scattered result slots, so the queries were resolved almost one by one
(637,729 resolves for 662,385 queries in a window) -- a resolve per draw stalls the GPU. Pictures unchanged (0.938-0.939,
as without), so the identification held on a still head. The second version takes each list's query indices in runs
of 256 (one resolve per run, results found through the draw's last index); it is built but its measurement did not
run: three launches in a row stopped at Windows refusing the game the foreground (the launcher's focus check), the
harness's environment, not the plugin.

### Single-pass ceiling on the CPU (`vr_bench ab --etw-cpu`, `build/vr-bench/*-ab-cpu-right-eye-draws`)

What a single pass done at the hook could save on the CPU: the right eye's RenderElements draws not sent to the driver
(`CyberpunkVR_GeometryCostProbe=146`, the right half's draws skipped; a diagnostic -- the right eye is left without
its G-buffer), ABBA x2 with wpr CPU sampling of every window, ms of CPU per frame summed over the threads:

| module | all draws | right eye's draws skipped | difference |
|---|---|---|---|
| nvwgf2umx.dll (NVIDIA driver) | 23.96 | 21.44 | **-2.5** |
| D3D12Core.dll | 2.16 | 2.09 | -0.07 |
| cyberpunkvrport.dll | 9.05 | 9.34 | +0.3 (noise: the hook still runs for every draw) |
| Cyberpunk2077.exe | 76.9 | 75.8 | inside the spin-wait noise |

- A hook-level single pass removes the driver's work for the right eye's G-buffer draws: ~2.5 ms of CPU a frame,
  spread over the worker threads that record those lists in parallel, so the frame's critical path gains a fraction
  of it. The engine's own per-eye preparation (instance records, descriptor tables, constants) stays: the engine still
  prepares and issues the right eye's draws, a hook can only drop them. Removing that needs the engine's code.
- With the 0.74 ms GPU ceiling above, that is what single pass is worth on this machine; on a CPU-bound one the driver
  share matters more (the same 2.5 ms of driver work, more of it on the critical path when fewer cores record).

### How the engine sends the two eyes (L/R pair probe, `vr_bench run --pairs`)

`src/Stereo/StereoPairProbe.cpp` (marker `cvr_pair_probe.txt` in the plugin's log folder at startup) keeps every
list's bound root parameters and records each RenderElements draw. 32 left/right batch pairs of equal length, 6,205
draws compared one by one:

- pipeline, root signature, index and vertex buffers: identical between the eyes;
- every root parameter is a descriptor table (no root CBVs), and the tables differ per draw (params 0, 2, 3 in
  100 % of the draws, 1 in 82 %, 8 in 53 %, 4 in 37 %): each eye's draw gets its own descriptor tables;
- the draw ARGUMENTS differ in 67 % of the draws.

The differing arguments are only the start instance (`argDiff 2.4`). Whether the differing tables carry per-eye data
was settled next by substitution: it is only the camera (next section), so single pass IS possible at the hook.

### Single-pass stereo with D3D12 view instancing (implemented, `CyberpunkVR_SinglePass`)

**Why it is possible.** A feasibility A/B (`vr_bench ab --pairs`, `build/vr-bench/20261105-103334-ab-sp-substitute`)
gave every right-eye RenderElements draw ALL its descriptor tables and its start instance from its left partner except
the camera tables, root parameters 0 (vertex) and 18 (pixel): the right eye's picture stayed the same (mean difference
to control 2.9 / 2.6 against 3.9 between two control windows); taking the camera tables as well broke it (32.0). The
root signature (logged by the probe) is 36 descriptor tables, 18 per stage; parameters 0 and 18 hold the CBVs b0..b3
(GlobalShaderConsts, CameraShaderConsts, ENV_PROBES, Custom*Consts). Inside one eye's batch table 0 changes 2 times in
9,569 draws, table 18 in 4 %.

**Shaders** (`src/Render/ViewTwinIr.cpp`, `ViewTwinCompiler.cpp`). dxcompiler.dll disassembles a shader in the game's
process; every b0..b3 load is read a second time from the same register in space 10 and each extracted value becomes
`select(SV_ViewID == 0, space 10, space 0)`; DXC's `-viewid-state` pass, its assembler and dxil.dll's validator make a
signed container. Twins are cached on disk (`cache/view_twins/`, keyed by the shader's hash and the recipe). The 262
RenderElements shaders of a census (127 VS, 135 PS, all SM 6.0) rewrite and validate, ~6 ms each
(`tools/vr_bench/view_twin_test.cpp`).

**A runtime rule found on the way.** There is no D3D12 debug layer on this system (and the Agility SDK's would not
load), so `tools/vr_bench/view_pso_test.cpp` recreated refused pipelines offline and synthetic shaders isolated the
cause: with view instancing, a pixel shader that reads SV_IsFrontFace cannot be linked with a vertex shader writing
outputs the pixel shader does not declare (the engine's SV_ClipDistance) -- CreatePipelineState returns E_INVALIDARG,
while the same pair without view instancing is fine (40 of 40 such pairs refused, 93 of 93 others accepted). The pixel
twin is therefore made for its vertex shader: the missing outputs are declared as (unread) inputs, the system inputs
moved behind them, elements in row order (a `<ps>.link` marker; twins `<ps>_<vs>.r2.dxil`). 0 refusals since.

**Pipelines** (`src/Render/SinglePassPipelines.cpp`). The material root gets a twin with parameters 36 (vertex) and 37
(pixel) = b0..b3 in space 10. Every pipeline created with it has its description owned at creation (~4,800 per
session); its twin is built by three background threads at first use (~280 in the benchmark scene, 2.6 s of builder
time with a warm disk cache): the twin shaders, the twin root, two views -- view 0 on viewport 0, view 1 on viewport
1, the same render-target slice (the side-by-side frame). When a hidden-area twin replaces a stage, the view twin is
built from that replacement.

**Draws** (`src/Stereo/SinglePass.cpp`). A left-eye RenderElements draw is kept, not sent. Its right partner -- the
same place in the right batch with the same pipeline, the index buffer and the vertex buffers its input layout reads,
topology, arguments but the start instance, stencil reference, blend factor and VRS state -- is sent ONCE: the twin
pipeline on the twin root, the right eye's tables as the engine bound them, the left eye's camera tables in 36/37,
two viewports and scissors (left, right). A partner that differs gets its own left draw just in front of it. Every
command that reads or writes what the draws produce (barriers, copies, clears, other render targets, queries, Close,
ExecuteIndirect, ...) first sends each kept left draw with its own state; the left eye's draws keep their order
throughout. While left draws are kept, the engine's state calls are held and the next draw gets the difference
(`CyberpunkVR_SinglePassLazy`: without it the left batch's state calls still reach the driver, +0.4 ms GPU against it).
`CyberpunkVR_SinglePass=2` is a replay test: every left draw kept and sent in front of its partner through the flush
path, no view instancing -- the picture stays the control's.

**Result, final build** (`vr_bench ab --label sp-final --etw-cpu --stereo`, ABBA x3, still head,
`build/vr-bench/20261105-122532-ab-sp-final`; CPU = wpr sampling of each window, ms of CPU a frame summed over the
threads, mean of the windows):

| variant | GPU p50 ms (min..max) | GPU p99 | FPS | CPU process | NVIDIA driver | plugin |
|---|---|---|---|---|---|---|
| control (`CyberpunkVR_SinglePass=0`) | 22.80 (22.71..22.86) | 24.25 | 44.0 | 159.1 | 22.9 | 10.4 |
| single pass | **22.38 (22.35..22.41)** | **23.84** | **44.7** | **150.2** | **14.7** | 11.7 |

GPU frame -0.42 ms p50 (ranges apart), the driver's CPU -8.2 ms a frame (both eyes' state calls and draws of the left
batch no longer reach it), the plugin's +1.3 ms (keeping and matching the pairs); 93 % of the pairs view-instanced
(7.46 M of 7.99 M draws a window). Each eye against control: mean difference 2.3-2.4 left, 2.4-2.5 right (one window
3.8, a steam cloud and walking NPCs that differ in that frame from every other window, the other single-pass windows
included), against 2.8-3.1 / 3.2 between control windows; stereo agreement 0.938-0.939 in every window.

An earlier run of the same build before the last two fixes (`sp-8`, ABBA x3): 22.73 (22.66..22.82) -> 22.45
(22.30..22.55) ms, p99 26.80 -> 24.12.

With the head turning fast (`--traj yaw_fast`, peak 283 deg/s, ABBA x2, stereo captured mid-turn,
`build/vr-bench/20261105-121546-ab-sp-yaw`): GPU p50 22.61 (22.61..22.62) -> 22.41 (22.23..22.58) ms; stereo
agreement 0.944 / 0.946 against 0.948 / 0.956 for control (both eyes' captures differ between windows by the
motion, single pass vs control by the same amount as control vs control). The same run's ETW CPU sampling showed
the NVIDIA driver 24.1 -> 14.3 ms of CPU a frame (summed over the threads), and also what cost too much in the
plugin: the hidden-area replacement check hashed a shader's bytecode for every kept draw (`PatchFor`, 13.4 ms a
frame); it is now asked once per pipeline (the final numbers above are after it).

The remaining 7 % are pairs whose eyes bind different vertex buffers or VRS rates; they are drawn as before. The
RenderElements node range itself measured +0.2 ms in `sp-8` (both eyes' geometry now inside the right batch's range,
the view-dependent vertex work per view); the frame is shorter.

### The port's own GPU work, and the first optimisation

Port GPU stages (`src/Render/GpuStageProfile.cpp`, 6 game frames): SubmitColor 90 us x 28, SubmitDepth 165 us x 14,
HudStyle 219 / HudBlur 131 / HudSprites 91 / HudSubmit 66 us x 6, CaptureMain / CaptureSecond ~98 us x 6. The XR
thread submits at the display rate (90 Hz) while the game draws ~43 frames a second, and every display cycle copied
the SAME captured frame into newly acquired swapchain images again (colour of both eyes and depth): ~0.8 ms of GPU
per game frame. The DLSS evaluates themselves, marked as pseudo-nodes: 2.91 ms (left), 2.99 ms (right).

**Fix (OpenXRFrameLoop.cpp, `LastCopy` in OpenXRManager.hpp):** a display cycle whose captured frame is the one
copied last (same capture serial and sources, no frame generation, no eyelids being drawn, same swapchains) skips the
acquire, both copies and the release; its projection layer names the same swapchains, and `xrEndFrame` composites
each swapchain's most recently released image (OpenXR 1.0/1.1 specification: xrEndFrame uses the image of the last
xrReleaseSwapchainImage of each referenced swapchain), which already holds that frame. The depth layer reuses the
last depth infos the same way. Any failed or partial cycle drops the record. `CyberpunkVR_XrReuseImages` (1)
switches it live; `CyberpunkVR_XrReusedCycles` counts the cycles it saved.

Same-launch A/B (ABBA x3, still, `build/vr-bench/*-ab-xr-reuse`):

| | GPU p50 | GPU p99 | FPS | stereo agreement / mean diff |
|---|---|---|---|---|
| copy every cycle | 23.86 ms | 26.11 ms | 41.6 | 0.938 / 1.80 |
| reuse | **23.24 ms** | **25.17 ms** | **43.2** | 0.938 / 1.77 |

Engine node time unchanged (22.53 / 22.54 ms): the saving is the port's copies alone (SubmitColor 28 -> 12 and
SubmitDepth 14 -> 6 per window: one copy per new game frame), 372 reused cycles per 8-second window.

### Head-turn hitches

The baseline snap90 spikes (p95 47 ms) did not come back: 3 x snap90 + snap180 in the next launch and snap90 as the
first turn of another launch all ran p99 24.1-25.2 ms. GPU queue timeline of those snaps: GPU busy per frame p50
21.5 ms, max 25.8-29.2 ms (the turned-to view costs up to ~5-7 ms more for a few frames), every long frame fully
busy (no CPU starvation).

### CPU headroom: lighter GPU settings for one launch (diagnostic)

The CPU frame time in these tables is the interval between real frames at the Present; under a GPU-bound frame it
equals the GPU time, and GameThread shows ~95 % of a core at 28, 43 and 70 FPS alike (it waits by spinning), so
neither says how much CPU work a frame has. Two launches with lighter DLSS settings (`--setting`, restored after):

| DLSS (per eye 2648x2864 out) | DLSS evaluate L / R | GPU p50, moving head | FPS | GPU util |
|---|---|---|---|---|
| Transformer Balanced (the user's) | 2.92 / 2.93 ms | 23.2 ms | 43 | 99 % |
| Transformer Ultra Performance | 4.64 / 4.72 ms | 20.2-20.5 ms | 49 | 100 % |
| CNN (Legacy) Ultra Performance | 1.48 / 1.47 ms | 14.1-14.3 ms | 70 | 99 % |

(`build/vr-bench/*-cpu-headroom-uperf`, `*-cpu-headroom-cnn-uperf`.) Every moving window stayed GPU-bound, even at
14 ms: the CPU work of a frame is below 14 ms, ≥ 9 ms under the 23 ms GPU frame of the user's settings. CPU-side
savings would not show in frame time on this PC until the GPU frame drops below ~14 ms. Two side findings: the
Transformer model costs MORE at Ultra Performance than at Balanced on this GPU (4.7 vs 2.9 ms an eye), and the nodes
whose cost did not move with the render resolution (DLSS evaluates and copies, ApplyDLSS, bloom/tonemap, CAS,
composition, final 2D, the unnamed full-frame pass, Present, DrawHUD) add up to ~9 ms of the 23. The first window
after a settings change had long frames (p99 43-98 ms, pipeline creation), and once a 7 s window got 1 frame (the game
stopped presenting while the XR thread ran on): `vr_bench` now records such a window under `stalls` and measures it
again.

### Stereo culling: what each eye sends (D3D12 pipeline statistics, `vr_bench run --pipeline-stats 0x33dda4`)

`CyberpunkVR_SceneGpuPipelineStats` (SceneGpuProfile.cpp) gives every profiled range on a direct list a
PIPELINE_STATISTICS query; per eye, RenderElements (G-buffer), still head, `build/vr-bench/20261104-233901-pipeline-stats`:

| per frame | left eye | right eye |
|---|---|---|
| primitives into the clipper (CInvocations) | 1,900,513 | 1,900,513 |
| vertex shader invocations | 2,456,481 | 2,456,481 |
| net share the clipper removed (1 - CPrimitives / CInvocations) | 32.5 % | 35.5 % |
| pixel shader invocations | 5,339,800 | 4,708,496 |

- The two eyes send exactly the same geometry: one scene traversal and one culling result for both (the engine culls
  with MAIN's camera, which the port gives the union of the two lens windows, `include/Stereo/EngineStereo.hpp`), and
  each eye's batch is re-sent with that eye's camera.
- About a third of the primitives are removed by each eye's clipper (a NET share and so a lower bound: Microsoft's
  D3D12_QUERY_DATA_PIPELINE_STATISTICS says CPrimitives can be larger or smaller than CInvocations, a clipped primitive
  being split into several or culled whole). Most are the out-of-view triangles of objects that are
  in view (ground, buildings): object culling keeps those whatever the frustum. What per-eye object culling could drop
  is the objects lying wholly in the strip only the other eye sees; their whole cost is bounded by the right eye's
  IA/VS/setup ceiling above (0.74 ms for ALL its draws), and the strip is the narrow nose-side difference of the two
  windows. Per-eye culling would also need the engine's per-object bounds inside its render proxies (D3D12 sees only
  draws), so it was not built.

### Shared lighting: the tiled light pass for both eyes in one dispatch (implemented)

The engine's tiled light shaders (78 permutations, `tools/asym_tests/stereo_ssr_merge_shaders.py` LIGHT_PASS) are
already side-by-side aware: they read nothing of the eye's camera block but its eye number (b1 row 53 is their only
b1 read), keep both eyes' pixel->world inverses in b12 (rows 77-80 left, 97-100 right, first-person beside them) and
pick them by that number. Every permutation is still dispatched once per eye over the whole frame's tile list, the
other half's tiles leaving after a tile and a depth load.

`tools/asym_tests/stereo_light_merge_shaders.py` builds twins (`src/Shaders/stereo_light_merge`, group 16 of
Render/HiddenAreaShaders.cpp, an ALTERNATE of each shader's stereo_merge twin, `CyberpunkVR_StereoLightMergePatch`,
on) in which every use of the eye number becomes the pixel's own eye (left or right of the shader's own `leftWidth`;
0 stays 0, the mono frame), the view origin is the pixel's eye position (that eye's inverse's depth column, b12 row 79
or 99, over its w), and the right eye's dispatch returns right after its group barrier: the left eye's dispatch lights
every tile with its own eye. A permutation reading any other b1 row would be skipped (none did: 78 of 78 built, DXIL
validator passed). The twins are created with the original pipelines (created at their first binds they made one
50 ms frame).

Same-launch A/B (ABBA x3, `build/vr-bench/*-ab-light-merge`, `*-ab-light-merge-yaw`):

| | RenderLightBuffers | GPU p50 | GPU p99 | FPS | stereo agreement / mean diff |
|---|---|---|---|---|---|
| still, per-eye dispatches | 2.636 ms | 22.84 | 25.03 | 44.0 | 0.938-0.939 / 1.71-1.92 |
| still, one dispatch | **2.325 ms** | 22.78 | (first window: pipeline creation) | 42.0 | 0.938-0.939 / 1.68-1.86 |
| yaw_fast, per-eye dispatches | 2.587 ms | 22.57 | 24.03 | 44.3 | |
| yaw_fast, one dispatch | **2.337 ms** | 22.48 | 23.71 | 44.6 | |

The pictures: `stereo_check --compare` between the variants' captures (per eye: mean 2.3-3.3/255, 1.3-3.2 % above 16)
is the same as between two captures of one variant (2.6-3.5, 1.5-4.0 %): frame-to-frame change (rain, animation),
not the lighting. -0.25 to -0.31 ms of GPU per frame in the pass; the frame-level p50 moves inside its window spread.

### SSR and AO: no work repeated between the eyes

Checked for the light pass's pattern (a per-eye dispatch over the whole frame that drops the other eye's half): the
SSR passes dispatched once per eye (trace `b76fb6b3b79f4925`, `9c9679515ec51bd3`, `e5741046a837b9cc`, classification
`bdf51af7acfab45e` at 38 x 41 groups = one eye's half) read no eye camera block at all (b0, b6, b12 only) and work on
their own eye's tiles: nothing to merge. The cone AO is one dispatch over both eyes, every pixel its own cone. What
they cost at High is per-pixel work at the render resolution (see SSR and AO at High).

### Shared shadows and lighting: what already runs once (GPU per node, final run)

Engine stereo already shares everything that does not depend on the eye: shadow cascades (RenderShadowCascade 0.04 ms,
ClearShadowCascades), local and distant shadow maps (0.22 / 0.01), clustered light culling (0.09), the volumetric fog
volume (0.47), GI (0.30), reflection probes (0.21), clouds, sky and water simulation run ONCE a frame (whole-frame or
compute ranges, no per-eye split). Per eye only what is per pixel: light integration (0.94 / 0.99 ms), the screen-space
shadow mask (0.11 / 0.11), the fog overlay (0.08 / 0.09), light channel volumes (0.03 / 0.03). The tiled light pass
(RenderLightBuffers 2.6 ms) is one indirect dispatch per eye per permutation, each over the whole frame's tile list
with the other eye's tiles leaving after a tile and a depth load -- merged into one dispatch, above.

### The port's own copies

Port GPU work per game frame (6 frames, final run): HUD panel ~0.51 ms (style 219, blur 131, sprites 90, submit 65 us),
the two eye halves copied out of the backbuffer at its Present (97 us each), each copied into its XR swapchain image
once (90 us each), depth 164 us. The capture copy stays: the XR thread copies when its display cycle comes, after the
backbuffer has gone to the DXGI Present (in the flip model DWM composes straight from the back buffers, Microsoft Learn
"DXGI flip model") and the engine draws into it again a few frames later; reading it then from another thread would
need ordering against both, so the port copies it inside the Present call. Copying straight into
acquired swapchain images at the Present would save the second copy (~0.19 ms) but moves swapchain acquire / release
out of the XR thread; not done.

### CPU by module and function (ETW sampling, `vr_bench run --etw-cpu still`)

`tools/vr_bench/etw_cpu.py`: wpr's CPU profile over one trajectory window (1 kHz per core; calibrated: a 3.0 s busy
loop = 3015 samples), converted with tracerpt and counted for the game's threads, the port's samples resolved per
function through its PDB. `build/vr-bench/20261104-230949-cpu-etw`, still head, 353 frames, ms of CPU per frame
summed over cores:

| | ms / frame |
|---|---|
| whole process | 153.6 (redDispatcher workers 125.6, GameThread 21.9 -- both mostly waiting by spinning) |
| Cyberpunk2077.exe | 74.9 |
| nvwgf2umx.dll (NVIDIA driver) | 23.0 |
| ntoskrnl.exe / ntdll.dll | 22.7 / 12.7 |
| **cyberpunkvrport.dll** | **9.1** (no function above 0.55: the draw hook 0.55, game-thread dequeue detour 0.45, descriptor copy hook 0.38, Lua VM ~0.35, CBV creation hook 0.20, VRIK 0.19, ...) |

The port's CPU is spread over ~15 worker threads and the frame is GPU-bound with ≥ 9 ms of CPU headroom, so none of
it is in the frame time. Two per-draw costs of switched-off features were removed anyway (the draw hook built and
cleared a 110-byte lens-velocity pass before asking whether the pipeline is a glass; the shader census and the layout
probe were called with their node lookups while both are off): `PrepareLensVelocity` 0.142, `shader_census` 0.133 and
`hidden_area::shaders::Command` 0.076 ms/frame left the profile (`*-cpu-etw-fastpath`, stereo unchanged
0.939 / 1.78). Whole-process totals move ±10 % between launches (153.6 vs 171.4 ms/frame here, every module up
alike), so only the functions that vanished are evidence, not the module totals.

### Where the GPU waits (queue timeline, `build/vr-bench/*-timeline-idle`)

Every batch on the game's queues timed, idle stretches between Presents split by `tools/hitch_tests/gpu_queue_timeline.py`
(`run --timeline-on`): moving head, 22.6 ms frames, the GPU busy **98.2 %** of the frame. Idle 0.40-0.42 ms a frame
in ~18 stretches (median 6 us): all "held" (batch submitted before the stretch began), none "starved" (the CPU never
submitted after the GPU had gone idle). The CPU submits about a frame ahead (submission to start: median 21.7 ms).
21 submissions / 77 command lists a frame on the direct queue, 3 on the async compute queue (0.9 ms busy, overlapped).
Nothing to win in CPU scheduling here: the GPU is fed; the idle is the gap between batches.

Measurement artifact found on the way: the window that carries the FIRST timeline request of a launch runs at 25-26 FPS
for seconds (SM clock 1190-1320 MHz instead of 2040, GPU frames from 102 ms down to 22 over ~3 s) -- in every launch
with a timeline on its first window and in none without. The still rows of `*-diag-lights-snap`,
`*-cpu-headroom-uperf`, `*-cpu-headroom-cnn-uperf` and `*-timeline-idle` are that, not the renderer (the tables above
use their moving windows). `vr_bench run` now builds the timeline with a small request during the warm-up and waits
for two 2 s windows with frame p50 within 4 % and SM clock within 3 % before the first trajectory.

### SSR and AO at High (`vr_bench run --setting ScreenSpaceReflectionsQuality=High --setting AmbientOcclusion=High`)

The user plays with SSR and AO off; these runs switch both to High for one launch (`UserSettings.json`, restored
after the game closes) to see what the screen-space passes cost in stereo. `build/vr-bench/20261104-221411-ssr-ao-high`:

| trajectory | FPS | CPU p50/p95/p99 ms | GPU p50/p95/p99 ms | 1 % low |
|---|---|---|---|---|
| still | 28.2 | 35.35 / 36.25 / 37.46 | 35.24 / 36.15 / 37.53 | 26.6 |
| yaw_slow | 27.7 | 35.66 / 38.02 / 38.42 | 35.61 / 38.24 / 38.90 | 25.9 |
| yaw_fast | 28.6 | 35.00 / 36.35 / 37.39 | 34.97 / 36.46 / 37.50 | 26.4 |
| translate_lr | 27.7 | 35.77 / 38.10 / 38.49 | 35.64 / 38.54 / 39.19 | 25.6 |
| lean_in | 27.0 | 37.11 / 37.97 / 38.03 | 36.92 / 38.14 / 38.76 | 26.0 |

GPU-bound (util 99 %). Stereo agreement unchanged from SSR/AO off (0.936-0.961, mean diff 1.35-1.89/255). Per node
(12 frames): **DrawConeAO 8.56 ms**, ScreenSpaceReflections 3.72 ms, the rest as before (+12 ms over the off
baseline in total).

- DrawConeAO: the trace dispatch (`1381eb9e608485d7`) is 150 x 81 groups of 16 x 16 threads, the full side-by-side
  render resolution (2 x 1197 x 1294), 15 directions x 10 depth gathers a pixel. One dispatch for both eyes; every
  pixel is a pixel of one eye, nothing is traced twice.
- ScreenSpaceReflections: the trace (`b76fb6b3b79f4925`) works on eye-local tiles; 3.7 ms is the cost of the
  reflections of two eyes at this resolution.

Same-launch A/B of the port's corrections in these passes (ABBA x2, `build/vr-bench/*-ab-ssr-ao-twins`):

| variant | DrawConeAO | GPU p50 |
|---|---|---|
| base (port twins) | 8.64 ms | 35.5-37.4 |
| AO twin off (engine shader) | 12.45 ms (+3.81) | 38.5-38.7 |
| AO two-halves flag off | 13.02 ms (+4.37) | 39.6-39.9 |
| SSR twin off | SSR 3.81 ms (+0.10) | 35.7-37.1 |

Under stereo the port's AO correction twin is already 3.8 ms cheaper than the engine's own shader (measured; why the
engine shader costs more there was not broken down).

**Rejected: skipping half of the AO dispatch.** In the AO twin the depth pixel of thread x is 2x mod W, which
suggested that the middle half of the threads repeats the outer half. A twin variant (`CyberpunkVR_EngineStereoAoFold`,
row 14.y bit 2) returned early for threads in [W/4, 3W/4). It saved 3.2 ms (DrawConeAO 8.29 -> 5.06 ms, GPU p50
35.70 -> 32.25 ms, ABBA x3) but broke the image: stereo mean diff 2.0 -> 36-40/255, and `stereo_check --compare`
full vs fold in adjacent windows gave a mean difference of 26.3/255 (p99 81-86, 52-56 % of pixels above 16) against
4.9-6.5 between two windows of the same variant, concentrated in each eye's inner (nose-side) half, lower rows. The
middle threads' AO is read, the premise was wrong; the variant was removed and the twin restored.

### CPU audit by thread and engine function (sections 9 and 10, `build/vr-bench/20261105-133317-ab-cpu-audit`)

`vr_bench ab --etw-cpu` now also resolves the samples inside Cyberpunk2077.exe to functions: `etw_cpu.ExeFunctions`
reads `build/patchday/ida/2.5/functions.tsv` (start, end, name of 338,250 functions, exported from the IDA database
of this build) and the summary lists the busiest exe functions overall and per thread. Fast yaw, 4 windows, ms of CPU
per frame summed over cores:

| thread | ms / frame | what the exe does there |
|---|---|---|
| redDispatcher workers | ~171 | job system: `sub_1402732D0` 12.0 (waits for jobs: WaitForSingleObject / ReleaseSemaphore around the job-scope allocator), `sub_1422CA420` + `sub_1422CA300` 3.3 (SwitchToThread spin loops, 200-400 callers each), shared-mutex locks `sub_14032A844`, `sub_1402782A4`, `sub_1403313BC` 3.4, memset / memmove / strncmp 3.8 |
| GameThread | ~20 | waits on the job dispatcher: `sub_14026D9A0` (FlushCounter) 0.54, `sub_14026DE10` (semaphore) 0.59 |
| redIOWorker | ~1.1 | file I/O |
| D3D background threads | 0.6-1.8 | runtime / driver |
| the port's twin builders | 0.4-1.4 | single-pass view twins (below normal priority, finish after a minute of a new scene) |

No engine function holds more than 12 ms of the ~200 ms summed, and the biggest are waits and spins of a
GPU-bound frame (GPU 22 ms, CPU threads idle-waiting for it). Animation, skinning (GPU compute), NPC and physics
jobs run inside the same redDispatcher workers; none appears as a separate hot function.

### Resource creation: the engine's streaming (implemented, `CyberpunkVR_StreamingPlaced`)

`CyberpunkVR_CreationCounts` counts every committed and placed resource, descriptor heap, pipeline and root
signature created during a window, which module asked (return address), and the time spent inside the runtime
and driver for each creation. Steady play creates no descriptor heaps or root signatures and 13-200 pipelines a
window, but a lot of resources, ALL asked for by Cyberpunk2077.exe (the port creates none):

| window (`*-ab-create-time-*`) | committed | placed | runtime + driver time, committed / placed |
|---|---|---|---|
| still, 8 s, 359 frames | 189-355 (0.5-1.0 a frame) | 1,011-1,529 | 0.15-0.28 / 0.04-0.06 ms a frame |
| fast yaw, 6 s, 271-273 frames | 1,903-1,966 (7 a frame, 5 MB) | 8,236-9,592 (33 a frame, 7.6 MB) | 2.1-2.2 / 0.44-0.49 ms a frame |

Head motion makes the engine stream; ~3 ms of CPU per committed creation (kernel and driver: new memory mapped
and zeroed) against ~0.13 ms per placed one. The call stacks (`CyberpunkVR_CreationStacks=1`, the busiest stacks to
the log) show the engine allocating through AMD's D3D12 Memory Allocator (D3D12MA: `sub_140349254` = CreateResource:
try the block vector, fall back to a committed resource; E_NOINTERFACE / E_INVALIDARG returns as in D3D12MA) and
asking for a dedicated (committed) allocation by size:

- textures, `sub_140347F74`: `ALLOCATION_FLAG_COMMITTED` for render-target / depth / UAV textures, and for every
  texture over 256 x 256 texels (`cmp ecx, 10000h; jg` at exe+0x3480EC);
- buffers, `sub_1403445AC` (GpuApi buffer creation, 176-byte records): committed for every buffer over 256 KB
  (`lea eax,[r10-1]; cmp eax, 3FFFFh; jbe` at exe+0x3448E3);
- the upload buffers that feed them are placed (`sub_1422F2D00`, `sub_140342F48`: the "CopyQueueUpload" path).

Video memory was not the reason (DXGI: 6.2-6.5 GB used of an 11.3 GB budget): it is the engine's policy.
`CyberpunkVR_StreamingPlaced=1` raises the two limits to 16 M texels and 32 MB, so streamed textures and buffers are
placed into D3D12MA's blocks; render-target / depth / UAV textures stay committed (their own flag test is untouched),
and D3D12MA itself still makes anything over half a block committed. Only the two compare immediates change (the
original bytes are checked first; a stale or half-written immediate is still a valid threshold), applied at the first
submission and restorable at any time (`=0`).

| same-launch A/B | committed a window | their runtime + driver time a window | process CPU (ETW) | GPU p50 | stereo agreement |
|---|---|---|---|---|---|
| fast yaw, ABBA x3 (`*-ab-streaming-placed`) | 1,970-3,270 -> 36-45 | 0.77-1.26 s -> 0.06-0.08 s | 202.8 -> 198.2 ms a frame (ntoskrnl 30.2 -> 27.8, NVIDIA driver 16.3 -> 15.9) | 22.21 (22.00..22.50) -> 21.86 (21.78..21.97) | 0.945-0.992 -> 0.992 |
| 180-degree snaps, ABBA x4 (`*-ab-streaming-placed-snap`) | 1,094-1,413 -> 16-165 | 0.40-0.49 s -> 0.02-0.18 s | 158.2 -> 156.8 | 21.41 -> 21.42 | 0.939-0.953 -> 0.939-0.957 |

- The CPU cost leaves: committed creations -97 %, their runtime and driver time -90 % or more, kernel time with it.
  Placed creations take the same time per window as before (they were already the majority).
- GPU: unchanged over the snaps; over fast yaw p50 -0.35 ms and p99 27.3 -> 24.1, but the control's p99 comes from
  one window with a 34 ms frame, so the GPU gain is not claimed.
- Video memory over the snap run (DXGI, logged every ~17 s): 6.16-6.22 GB while placed, 6.21-6.48 GB while committed
  -- packing into blocks uses less, not more; no growth over the run.
- The pictures: the same scene with every texture at full resolution (no late or blurry mips), agreement in the
  control's range.
- On by default. The limits are the 2.5 exe's; another build fails the byte check and keeps the engine's policy
  (`CyberpunkVR_StreamingPlacedState` = 0xFFFFFFFF and a log line).

## Reprojection rate (`repro%` in every `vr_bench run` window)

`CyberpunkVR_XrProjectionCycles` / `CyberpunkVR_XrRepeatCycles` (OpenXRFrameLoop.cpp, one relaxed add each per
display cycle): the share of 90 Hz display cycles that showed the game frame the previous cycle had already shown.
`build/vr-bench/20261105-094151-reprojection`: still 51.5 %, yaw_fast 50.8 %, snap90 49.7 % at 43.6-45.4 game FPS --
every game frame is shown about twice, the second time reprojected by the runtime (90 / 44 = 2.05). Head-to-photon
latency on a device was not measurable here (no headset; the simulator presents to a window).

## Other scenes: the optimisations off vs on (`vr_bench ab --save`, ABBA x3, still head, stereo per window)

Variants: `CyberpunkVR_XrReuseImages` and `CyberpunkVR_StereoLightMergePatch` both 0 (off) or both 1 (on).

| scene (save) | GPU p50 off -> on (window range) | GPU p99 | FPS | RenderLightBuffers | stereo agreement / mean diff |
|---|---|---|---|---|---|
| Japantown street, day, a large hologram overhead (ManualSave-3) | 20.03 (19.84-20.39) -> **19.44** (19.34-19.58) | 22.92 -> 22.54 | 49.5 -> 51.1 | 0.895 -> 0.834 | 0.946 / 8.3-15.9 in both |
| Arasaka industrial park, night (ManualSave-407) | 21.76 (21.30-22.65) -> 21.47 (20.81-22.64) | 23.34 -> 23.79 | 46.3 -> 46.8 | 1.584 -> 1.472 | 0.939-0.940 / 1.32-1.36 in both |

- Japantown: -0.59 ms GPU p50 with no overlap of the windows. The high mean difference is the same with the
  optimisations off and on and sits only on the transparent hologram: the depth reprojection check uses the opaque
  depth behind it, so a transparent layer at another depth shows as a mismatch even when both eyes draw it right (a
  limit of the check, not a finding about the renderer).
- Industrial park: the window spread (NPCs moving) is larger than the change; the pass itself -0.11 ms, profiled
  nodes 20.46 -> 20.13 ms.
- Not measured: ManualSave-157 (Phantom Liberty tunnels) and ManualSave-110 (prologue) did not reach the loading
  screen's Continue within 300 s in the launch tool (QuickBoot); the launches were closed.

## Stereo under other eye geometry (`vr_bench run --stereo-sweep`, `build/vr-bench/20261105-092008-stereo-sweep`)

The simulator's live IPD and FOV commands, one stereo capture each on the still head (stereo_check: the separation
of the two eye positions the port rendered with, depth reprojection agreement, colour difference, depth error):

| geometry | rendered separation | agreement L->R | mean diff | depth error p95 | eye FOV in the capture (L / R, rad: l, r, u, d) |
|---|---|---|---|---|---|
| IPD 58 mm | 57.9 mm | 0.944 | 1.61 | 0.00114 | Quest 3 |
| IPD 70 mm | 69.9 mm | 0.933 | 1.55 | 0.00108 | Quest 3 |
| IPD 64 mm | 64.0 mm | 0.938 | 1.55 | 0.00110 | -0.94, 0.70, 0.78, -0.95 / -0.70, 0.94, 0.78, -0.95 |
| symmetric FOV | 64.0 mm | 0.944 | 1.70 | 0.00107 | -0.86, 0.86, 0.85, -0.95 (both) |
| wide asymmetric FOV | 64.0 mm | 0.943 | 1.85 | 0.00095 | -1.02, 0.78, 0.90, -1.00 / -0.78, 1.02, 0.90, -1.00 |

The port follows IPD and FOV changes live (the rendered separation is the commanded IPD to 0.1 mm; the captured
windows are the commanded ones plus the port's small margin), and stereo agreement stays where it is at the default.


## Goal sections 6-10: status by item

Every item of the goal, what was done with it and where the evidence is. "Done" = implemented, measured same-launch,
pictures checked; "measured" = examined with numbers, no change made (reason given); "not done" = not examined or not
possible from the plugin, said so.

**6. Stereo-aware culling and GPU-driven rendering**

| item | status | evidence |
|---|---|---|
| combined stereo frustum culling | in place (engine stereo), measured | one culling result for the union of the lens windows: both eyes send the same 1.90 M primitives (Stereo culling) |
| shared CPU visibility processing | in place, measured | the same section: one scene traversal for both eyes |
| per-eye visibility masks | measured, not built | ~50 % of each eye's G-buffer draws leave no sample, the eyes agree (49.5-51.6 %); objects only one eye sees lie in the narrow nose-side strip, bounded by 0.74 ms of IA/VS for ALL right-eye draws; needs the engine's object bounds (Draw calls, Stereo culling) |
| GPU occlusion culling | **prototyped** (off by default) | per-draw occlusion queries (Draw calls) and a working prototype: one query per view-instanced draw (conservative across the eyes), predication on the last result, re-test every 8th frame (GPU occlusion culling prototype). First version cost 35-50 ms of GPU in per-draw resolves; the batched version awaits its measurement. Not shippable either way: a draw coming into view is late by up to the re-test interval (pop-in) |
| compute-based culling / GPU-driven command generation / GPU scene data | not possible from the plugin | the geometry is CPU-submitted per draw; a GPU-driven path replaces the engine's scene submission (closed code) |
| D3D12 ExecuteIndirect | measured | the engine already issues lighting, SSR and other passes through ExecuteIndirect (scene profile marks them) |
| minimise repeated geometry preparation | **done (single pass)** | 93 % of the G-buffer draw pairs drawn once for both eyes; the NVIDIA driver's CPU 22.9 -> 14.7 ms a frame, GPU p50 22.80 -> 22.38 ms (Single-pass stereo) |
| meshlets / mesh shaders | measured: the engine has no mesh-shader path | MeshShaderTier 1 available; every recorded G-buffer draw is a vertex-shader DrawIndexedInstanced / DrawInstanced; every pipeline stream is checked for an MS subobject at creation (`CyberpunkVR_CreationCounts[14]`): 0 of 4,904 graphics pipelines since launch (`*-ab-mesh-census`). Meshlets would need the engine's meshes rebuilt into meshlets and its submission replaced -- the same GPU-driven rewrite as above |
| objects seen by one eye only kept | checked | single pass draws both views of every pair, nothing is culled per eye; stereo agreement unchanged |
| LOD selection, pop-in, streaming, consistency in head motion | checked | each pair shares index/vertex buffers (same LOD both eyes); stereo checks with yaw, snap, combo, translation, lean: agreement 0.92-0.95, no missing or doubled geometry; single pass in fast yaw: agreement 0.944-0.946 vs 0.948-0.956 |

**7. Shared lighting and shadows**

| item | status | evidence |
|---|---|---|
| directional shadow maps | already shared | RenderShadowCascade once a frame, 0.04 ms (Shared shadows and lighting) |
| local light shadow maps | already shared | 0.22 ms once a frame; 41.7 % of their casters leave 1-64 samples (the Draw Calls Reducer small-caster idea; not built: ~5 % of the draws) |
| reflection probes, GI, clouds, sky, water | already shared | 0.21 / 0.30 ms once a frame |
| world-space lighting data, clustered light culling, GPU light lists | already shared; light-cull twins per eye bounds | clustered culling 0.09 ms once; the light and probe culling twins use each eye's projection scale |
| tiled light pass | **done** | both eyes lit in one dispatch: RenderLightBuffers 2.64 -> 2.33 ms (Shared lighting) |
| shared radiance cache | measured: the world-space GI is already shared | the engine's global illumination (node GlobalIllumination, world-space data) is one compute range a frame for both eyes, 0.28 ms (final benchmark profile: L 0 / R 0 / compute 0.28); there is no other radiance cache in this rendering mode (ray tracing off) to share |
| GPU skinning | already shared | ComputeSkinningAndTangentUpdates: one dispatch a frame |
| volumetric lighting data | already shared | the fog volume once (0.47 ms); only the per-pixel overlay is per eye (0.08 / 0.09) |
| shared vs per-eye split | documented | per eye only per-pixel, view-dependent work: light integration, shadow mask, fog overlay, light channels |

**8. VR rendering path**

| item | status | evidence |
|---|---|---|
| redundant GPU memory operations | **done** | XR image reuse: no copy of an already submitted frame, -0.6 ms; remaining copies listed with costs (The port's own copies) |
| G-buffer / bandwidth | **done in part** | single pass shares the G-buffer geometry pass (above); formats and targets are the engine's |
| expensive pixel passes | measured | per node and per pipeline; SSR / AO at High are per-pixel work; the AO half-dispatch shortcut rejected (broke the image) |
| transparent objects, decals, particles, volumetrics | measured | per node costs (scene profile); single pass on every node instead of RenderElements only (`CyberpunkVR_SinglePassNode=0`): GPU p50 22.89 against 22.77 for RenderElements only and 23.08 for control (ABBA x2, `*-ab-sp-allnodes`) -- the other passes add ~5 % kept draws and more replays (render-target changes and barriers between their eyes' batches), so RenderElements stays the default |
| VR shader variants for expensive materials | **done** for the light pass and the G-buffer | light-merge twins (78), view-instanced material twins (~280 in the scene) |
| reflections, AO, shadow filtering, denoising | measured | no work repeated between the eyes (SSR and AO); shadow filtering is the engine's per-pixel mask |
| Forward+ / hybrid | not possible from the plugin | the engine's deferred path cannot be replaced at the D3D12 boundary |

**9. CPU**

| item | status | evidence |
|---|---|---|
| game thread, render/worker threads, synchronisation | measured | ETW by thread and exe function (CPU audit): GameThread ~20 ms a frame, mostly waiting on the job dispatcher (FlushCounter, semaphore); redDispatcher workers ~171 ms summed, top: job waits 12 ms, SwitchToThread spins 3.3, shared-mutex locks 3.4 -- waiting for a GPU-bound frame (>= 9 ms headroom) |
| draw call preparation, command list recording, descriptor management | **done (single pass)** | driver CPU -8.2 ms a frame; the port's draw-hook fast paths -0.35 ms; the plugin's own +1.3 ms for keeping the pairs |
| memory allocation | **done** | the engine's streaming: committed resources for textures > 256 x 256 and buffers > 256 KB, ~3 ms of kernel + driver CPU each, 7 a frame in fast yaw; now placed into the engine's own D3D12MA blocks: committed creations -97 %, process CPU -4.6 ms a frame (fast yaw) / -1.4 (snaps), video memory lower (Resource creation). The port's pipeline registry shares one copy of each shader (20 MB for ~4,800 pipelines) |
| garbage collection | measured | Lua GC already paced off the frame's path by the plugin's Lua host |
| animation / skinning, NPC updates, physics | measured | ETW per exe function and thread (CPU audit): they run inside the redDispatcher job workers, none is a hot function; the workers' top functions are job waits, spin loops and shared-mutex locks of a GPU-bound frame; skinning is a GPU compute dispatch |
| object streaming | **done** (resource creation) | streaming is head-motion driven (still: 0.5-1 committed creations a frame, fast yaw: 7); its committed creations moved into placed (above) |
| periodic spikes | **done** | 50 ms frames from creating twins at first bind (now at pipeline creation / in background threads); the harness's first timeline request; launch tool file I/O |
| work moved to background threads | done | view twins built by three below-normal builder threads, never on a recording thread |

**10. D3D12 render graph, synchronisation, scheduling**

| item | status | evidence |
|---|---|---|
| graphics and compute queues, async compute | measured | 21 submissions / 77 lists a frame on the direct queue, 3 on async compute (0.9 ms busy, overlapped) |
| fences, CPU/GPU synchronisation | measured | GPU busy 98.2 %; idle 0.4 ms a frame, all held, none starved; the CPU submits a frame ahead |
| resource barriers, state transitions | measured | `CyberpunkVR_BarrierCounts` (the engine's barriers by kind and the runtime + driver time), still head, `*-ab-barriers`, `*-ab-barrier-repeat`: ~600 ResourceBarrier calls a frame with 1,063 transitions (86 % whole-resource), 138 aliasing and 153 UAV barriers, no split barriers; 1.2 ms a frame inside the runtime and driver (~2 us a call, summed over the recording threads, not on the GPU-bound frame's path), so merging calls would win well under that. Single pass on / off: 600.3 / 596.6-600.9 calls, aliasing 138.0 / 137.7-138.4 -- it keeps every barrier (kept draws are sent before any barrier, copy, clear or target change). One launch measured under ETW sampling (`*-ab-barrier-time`) recorded 2,460-2,530 calls a frame in all four windows, the same mix and cost per call, 5.0-5.25 ms; two launches without ETW did not -- cause not found, the ETW module totals of that kind of window carry it |
| pipeline state switching | **done (single pass)** | while the left batch is kept its state calls do not reach the driver (lazy state); without it +0.44 ms GPU |
| command list submission | unchanged | the engine's submission order is kept |
| transient resources, render-target lifetime, descriptor heaps | measured, streaming part **done** | the engine's render graph already aliases transient resources in shared heap memory (138 aliasing barriers a frame, above); creation counters per window: no descriptor heaps and no root signatures created in play, 13-200 pipelines a window, the port creates no resources; the engine's per-frame creations are streaming (D3D12MA; upload buffers placed, large textures / buffers committed -> now placed, Resource creation); none of the 16 busiest creation stacks has a render-target, depth or UAV flag |
| present / reprojection | measured | depth submitted to the runtime; reprojection rate per window (~50 % at 44 FPS on 90 Hz) |

## Sources (official documentation this work relied on)

- Microsoft, D3D12 View Instancing Functional Spec (microsoft.github.io/DirectX-Specs/d3d/ViewInstancing.html): what a
  view-instanced draw is (one draw, SV_ViewID per view, tiers 1-3, tier 3 = "always occurs at the first shader stage that
  references SV_ViewID"). The pair probe's finding (each eye's draw has its own arguments and descriptor tables) is why
  it could not stand in for the engine's per-eye batches.
- Microsoft Learn, D3D12_FEATURE_DATA_D3D12_OPTIONS3 and the feature queries in `tools/vr_bench/gpu_caps.cpp`
  (ViewInstancingTier 3, SM 6.8, MeshShaderTier 1 on this RTX 3060).
- Microsoft Learn, "Timing (Direct3D 12 Graphics)": timestamp queries are bottom-of-pipe, ticks / queue frequency,
  GetClockCalibration correlates them with QueryPerformanceCounter -- the scene profiler, the port stage profiler and
  the queue timeline rest on this (and on its warning that timestamp clocks can drift when the GPU idles).
- Microsoft Learn, D3D12_QUERY_DATA_PIPELINE_STATISTICS: CInvocations = primitives sent to the rasterizer, CPrimitives =
  primitives rendered, "larger or smaller than CInvocations" after clipping -- hence the culling table's NET share.
- Microsoft Learn, "Indirect Drawing" (ExecuteIndirect, command signatures): how the tiled light and SSR passes are
  issued, and that a dispatch's group count comes from a GPU buffer the port does not rewrite.
- Microsoft Learn, "DXGI flip model": back buffers shared with DWM, composed without a copy.
- Khronos, OpenXR 1.1 reference pages, xrEndFrame: "only one image index is implicitly referenced per frame, the one
  corresponding to the last call to xrReleaseSwapchainImage" -- the XR image reuse; XR_ERROR_LAYER_INVALID for a
  swapchain without a released image -- why a failed cycle drops the reuse record.
- Microsoft DirectXShaderCompiler (dxc -dumpbin, DXIL validator via `build/single-pass-tests/Release/dxil_tool.exe`):
  every shader twin is disassembled, rewritten as DXIL, re-assembled and validated (container hash checked).
- Microsoft Learn, Windows Performance Recorder / tracerpt: the CPU sampling of `tools/vr_bench/etw_cpu.py`.

## Files and subsystems changed by this work (46c98879..HEAD)

- XR submission: `src/Runtimes/OpenXRFrameLoop.cpp`, `include/Runtimes/OpenXRManager.hpp` (image reuse, `LastCopy`).
- Shader twins: `src/Render/HiddenAreaShaders.cpp` (alternate group 16), `src/Shaders/stereo_light_merge/` (78 twins),
  `tools/asym_tests/stereo_light_merge_shaders.py` (their generator).
- Draw hook: `src/Stereo/CommandListCensus.cpp`, `src/Render/PhysicalOpticsShaders.cpp`,
  `include/Render/PhysicalOpticsShaders.hpp` (fast paths; viewport detail and eye bits for profiling).
- Profiling and diagnostics (off unless asked for): `src/Render/SceneGpuProfile.cpp`/`.hpp` (frames, eyes, pipeline
  statistics), `src/Stereo/StereoPairProbe.cpp`/`.hpp` (L/R pair records), `src/Framegen/Framegen.cpp` (frame time
  rings), `src/Framegen/FrameCapture.cpp`, `src/Framegen/Presenter.cpp` (stereo captures without frame generation),
  `src/Stereo/Dlss.cpp`, `src/Stereo/EngineStereoDlss.cpp` (DLSS pseudo-nodes), `src/Stereo/DeviceHooks.cpp`,
  `src/Hooks/SwapChain.cpp`, `include/Render/GeometryCostProbe.hpp`.
- Harness: `tools/vr_bench/` (vr_bench.py, trajectories.py, stereo_check.cpp, etw_cpu.py, gpu_caps.cpp, telegram.py,
  tests, README, simulator head-trajectory patch), `tools/hitch_tests/gpu_queue_timeline.py`, small fixes in
  `tools/quickboot_tests/launch_to_save.py` and `tools/single_pass_tests/`.
- Single pass: `src/Stereo/SinglePass.cpp`/`.hpp` (draws, state, flushes), `src/Render/SinglePassPipelines.cpp`/`.hpp`
  (twin root and pipelines, builders, disk cache), `src/Render/ViewTwinIr.cpp`/`.hpp` (the DXIL rewrite),
  `src/Render/ViewTwinCompiler.cpp`/`.hpp` (dxcompiler/dxil.dll in process), `src/Render/ViewInstancedPipeline.cpp`
  (classic description as a stream, stream description and edit for diagnostics), `src/Render/HiddenAreaShaders.cpp`
  (`BoundCode`), the hooks in `src/Stereo/DeviceHooks.cpp`, `CommandListCensus.cpp`, `Dlss.cpp`; tools
  `tools/vr_bench/view_twin_test.cpp`, `view_pso_test.cpp`; `vr_bench ab --prime`, array counters, census modes.
- Resource creation: `src/Stereo/DeviceHooks.cpp` (`CyberpunkVR_CreationCounts` by kind, caller and runtime time,
  `CyberpunkVR_CreationStacks` stack histogram, video memory to the log, `CyberpunkVR_StreamingPlaced`);
  `tools/vr_bench/etw_cpu.py` (exe samples by IDA function and thread), `vr_bench.py` (`EXE_FUNCTIONS`).
- Every runtime change has a live switch for rollback and A/B: `CyberpunkVR_XrReuseImages`,
  `CyberpunkVR_StereoLightMergePatch`, `CyberpunkVR_SinglePass` (0 off, 1 on, 2 replay test),
  `CyberpunkVR_SinglePassLazy` and `CyberpunkVR_StreamingPlaced` (the draw hook fast paths are behaviour-identical
  and have none).

## Remaining limitations and next candidates

- Single pass covers RenderElements (the G-buffer and its depth-only draws); other per-eye geometry passes (forward
  translucency, decals) still draw twice. The engine still prepares the right eye's draws (instance records, tables):
  single pass drops them at the hook, so the engine's own CPU work for them stays. 7 % of the pairs differ in vertex
  buffers or VRS rate and are drawn as before. The first launch builds the shader twins (~6 ms each, cached on disk).
- Per-eye object culling: needs per-object bounds from the engine's render proxies; ceiling < 0.74 ms GPU.
- SSR / AO: stereo-correct (port twins), no work repeated between the eyes; at High their cost is per-pixel work.
  With SSR at High, the fast-yaw stereo check rose at the image's leading edge (left columns 3.5 vs 1.1-2.0 without),
  where both eyes see reflective glass; reflections differ between the eyes by nature and the capture fell at another
  point of the turn at the lower frame rate, so it is not taken for a defect -- an SSR on/off comparison at the same
  head pose is still to be made.
- The visibility mask of the headset (XR_KHR_visibility_mask) is not used; the simulator does not provide one, so a
  pixel skip on it could only be validated in a headset.
- Port copies: straight copy into acquired swapchain images at the Present (~0.19 ms), HUD composite restricted to the
  sprites' areas (~0.1-0.15 ms): not done (risk vs gain).
- Scenes: H10 street in rain (the main benchmark), Japantown by day and the Arasaka industrial park at night (A/B
  only); an interior with close geometry and a high-draw-call scene still lack a save the launch tool can load.
- No real headset was available: head-to-photon latency on the device was not measured (the reprojection rate is,
  above).
- Game thread: the port's Lua mods take ~2.0 ms a tick in the city (plugin log `[lua] onUpdate`: p50 1.66, p99
  11.1 ms; Stereo 0.51, TacticalBelt 0.29, Reach 0.17, Weapon 0.14, VRIK 0.14, QuickBoot 0.10 -- the last only in
  launches the tools own). Their GC is already paced off the frame's path by the plugin's Lua host; with the frame
  GPU-bound and >= 9 ms of CPU headroom this is not in the frame time, but it is the next CPU item.