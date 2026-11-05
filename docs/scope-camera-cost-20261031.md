# What a camera scope costs (2026-10-31)

Players report 70-80 FPS dropping to ~55 with a camera scope (the RTT image in the lens). Measured in the
simulator (basketball court, engine stereo, no frame cap, no frame generation), Grad (4.25x, camera FOV
22.3 deg) drawn against holstered at the same spot:

| | holstered | Grad drawn |
|---|---|---|
| frame (median) | 13.1 ms, 76 FPS | 20.4 ms, 49 FPS |
| GPU busy (nvidia-smi) | 89 % | 74 % |
| scope view GPU (SceneGpuProfile, side 2) | - | ~2.2 ms/frame |
| eyes GPU (side 0) | | ~11.9 ms/frame |

GPU use goes DOWN with the scope: the frame becomes latency-bound on the CPU side.

**Fixed 2026-11-01**: the engine built the scope's render graph as one chain of 63 nodes; see "The scope's graph
was one chain" at the end (`xr_scope_graph_dag`, H10 Grad at the eye 26.2 -> 18.5-19.0 ms, GPU ~80 -> ~93 %).

## Where the time goes

- The scope camera is an RTT sub-scene with MAIN's full feature set (FrameGraph.cpp forces `f0 | main.f0`, the
  shadow cascades shared). Its render graph runs once per frame, as many node calls as MAIN's
  (`node_timeline.py`: sub_140332D5C 66 calls against MAIN's 84).
- The render scene chains every view it schedules on one handle (`scene+8`, ViewChainTimeline.cpp). The scope's
  graph therefore runs between MAIN's early nodes and MAIN's bulk. MAIN waits for it: holstered, its largest idle
  gap is 0.13 ms (5AA54C -> next). With the scope it is 11.5 ms (5AA54C -> BFDD24).
- Inside, the scope's long node groups run one after another, one worker at a time: ~0.8 nodes in flight, a
  12.3 ms chain carrying 8.3 ms of node work. Between groups the job timeline (`CVR_JOB_TIMELINE=1`) shows the
  engine's own stages: `RunRenderJob` (56 per frame), `CloseAndSubmitCommandLists_Close/Submit`. They are not
  idle waits.
- The port's own share is small. Worker stacks (`thread_stack_sampler.py` + `busy_profile.py`): workers are 32 %
  busy, and of that the plugin itself is 7.5 % (lock-free paths already, see `single-pass-ceiling-20261008.md`).
  The scope image resolve (CameraSourceResolve.cpp) is one small GPU list per frame.

## The scope's passes (second measurement, same save)

`node_timeline.py --level 2` records nested nodes too, and a scene pass's rtId (NodeDispatch.cpp). Holstered
11.9 ms (84 FPS), Grad drawn 16.2 ms (62 FPS), so +4.3 ms. The scope view's SceneDrv work is 5.0 ms/frame
over 33 passes, each called twice per frame like MAIN's: a short prepare at the frame start, then the work.
No pass dominates (`tools/hitch_tests/scope_passes.py`, ms/frame):

| pass content | ms |
|---|---|
| lighting: reflection probes, clustered light cull, light buffers, volumetric fog, integrate | 0.74 |
| forward: render elements, volumetric clouds | 0.42 |
| distortion, heat haze | 0.31 |
| GBuffer + AutoSpawnOnTerrain | 0.29 |
| sun cascades (two passes of RenderShadowCascade) | 0.49 |
| post (copy, histogram, gameplay FX) | 0.28 |
| ~27 more passes | 0.04-0.19 each |
| local shadow maps | ~0.01 |

Its execution window is 7.0 ms with 5.3 ms of node work: 0.77 nodes in flight, while MAIN runs 2.8-3.2 in
flight on 15-16 workers and does nothing during the scope's window. The per-view stage latencies (prepare,
occlusion, cull, collect, burst, tail; see `gpu-not-full-20261016.md`) dominate a view with short passes, so
the cost is the scope being a second view, not its draw count. Local shadows, the one pass the port could
lend without a camera dependency, cost almost nothing here (and their lend left trails for the user).

## What does not work

Running the RTT sub-scene beside MAIN was tried for VRCAM in `gpu-not-full-20261016.md` ("View overlap attempt"):
both graphs touch per-frame render state the engine only touches serially, and it crashed in
`DeclareCommonResourceAllocs`. The scope view has the same structure.

## What would

These change what the scope renders, or share work between the views. Both need agreement first
(no node cuts, no quality cuts):

1. Share MAIN's per-frame results with the scope view beyond the cascades: local shadow maps, light culling, sky
   and fog volumes. Each one removes a stage from the serial chain. This is research per stage, like ViewReuse.cpp.
2. A lighter feature set for the scope view, without the full screen-space and volumetric set. The scope
   image is 1024 px at 22 deg, and it shows less of these effects. This shortens the chain by those stages.

Tools: `scopeprof.py` (frames / gpu / cpu / audit) and `chainjobs.py` (jobs inside the scope's chain window) in
the session scratchpad, on top of `tools/hitch_tests/node_timeline.py` and the plugin's SceneGpuProfile.

## What was done (third measurement, Pacifica roof, uncapped, no frame generation)

Grad drawn against holstered: 11.1 ms holstered, 17.6-17.9 ms with the scope before these changes.

**MAIN's sun cascades for the scope** (`CyberpunkVR_OpticsCascadeLend`, default 1). The scope view ran under
`CyberpunkVR_VrcamFlagMode` 1 (MAIN's flags as they are), so its graph had its own `ClearShadowCascades` and two
`RenderCascade` passes over the shared atlas. It now gets feature bit 51 (FrameGraph.cpp): no clear and no cascade
passes. A drawn scope leaves MAIN's shadows unchanged while it redraws the shared atlas (same pose, time frozen), so
its work reaches the GPU before MAIN's cascades: at its turn the atlas holds MAIN's previous frame. Grading.cpp
(`optc_apply_scope`) replaces the scope's `cascadeMatrix[4]` and `cascadeVec[4]` (CSConstants rows 6-25, the only
rows that differ between the views) with MAIN's from the previous frame.

- Scope graph: 30 passes, SceneDrv 4.8 ms/frame, no cascade passes (before: 33-40 passes).
- Frame with the Grad: 16.9 / 17.1 ms with the lend, 17.6 / 17.9 ms without (alternating runs).
- The scope image, far (distant shadows) and near (cascade 0, the roof floor 2-4 m away): lend on vs off differs
  by 0.5 levels, the same as two shots with the same setting.

**G-buffer partitions for the scope** (`xr_scope_gbuffer_partitions`, default 0). The VRCAM partition trampoline
(StageReplay.cpp) takes a second key, the scope's camera name. Measured slower: 18.3 ms with partitions against
17.8 ms without (two runs each). The scope's G-buffer is 0.3 ms, and a view whose cost is its passes' serial stage
latencies gets slower with more passes. Available as a setting, off.

Running the scope's graph beside MAIN's instead of before it is still blocked by the shared per-frame render
state (`DeclareCommonResourceAllocs`, see "What does not work").

## Cascade lend reverted (2026-11-01)

The scope camera renders its own sun cascades again, as the user asked: feature bit 51 is no longer set on the scope
view (FrameGraph.cpp), and the `optc_*` lend of MAIN's previous-frame CSConstants rows is gone (Grading.cpp,
`CyberpunkVR_OpticsCascadeLend`). `xr_scope_gbuffer_partitions` stays (default 0).

Fourth measurement, Megabuilding H10 street entrance (ManualSave-490), uncapped, no frame generation:

| | frame (median) |
|---|---|
| holstered | 18.2 ms (55 FPS) |
| Copperhead (no scope) | 19.6 ms |
| Grad, camera scope | 26.6 ms (38 FPS) |

- Scope view GPU (SceneGpuProfile, side 2): ~1.5 ms/frame against ~14.8 ms for the eyes. The drop is CPU latency.
- `node_timeline.py --level 2`: MAIN's idle gap during the scope view (RenderRainMap -> SimulateOffScreenCPUParticles)
  is 12.0 ms. The scope's SceneDrv work is 7.1 ms/frame over 66 calls. This street has more in it than the court:
  local shadow maps 0.42 ms, cascades 0.51 ms. Also in the scope's window: `Synchronize` 0.76 ms and
  `SimulateOnScreenCPUParticles` 0.28 ms.
- Scope window (scratchpad scopewin.py): 13.4 ms long, 7.9 ms of node work, 5.5 ms with no scope node running.
  Of that, 4.3 ms are 13 gaps of 0.1-1 ms. The largest gaps follow `Synchronize` (1.46 ms/frame before the next
  SceneDrv), `PrepareDistantShadows` -> `Selection` (0.84), SceneDrv -> SceneDrv (0.68), DoCulling -> DoCulling (0.66),
  and PrepareCollector -> DoCulling (0.59).
- GameThread: the Optics mod's onUpdate costs 0.7 ms per tick with the camera scope (host log). It places the camera
  every frame: two FindComponentByName calls, the slot transform, the scope offset, SetLocalTransform and the fov.

## The submission chain was off (2026-11-01)

With the scope drawn the frame is limited by the render chain, not the game thread: the world tick ends ~17 ms after
Present and the game thread then waits ~7 ms for the render scene (`SCENE_WAIT`); holstered it waits 0. The scope's
largest gap, `Synchronize` -> next SceneDrv (1.46 ms/frame), is its Synchronize nodes waiting for their batches'
Close/Submit. That is what `xr_submit_chain` removes (src/Stereo/SubmitChain.cpp). It and `xr_defer_present` had been
0 since 2026-10-17-19, set off as suspects in the grey-materials hunt whose cause was async compute on the graphics
queue. Same launch, Grad drawn, live switches (scratchpad ablive.py, 3 rounds x 8 s, frame period from the Present
hook's counters):

| xr_submit_chain / xr_defer_present | frame | FPS |
|---|---|---|
| 0 / 0 (as shipped) | 23.85 ms | 41.9 |
| 2 / 0 | 22.01 ms | 45.4 |
| 2 / 1 | 21.66 ms | 46.2 |
| 1 / 1 | 21.47 ms | 46.6 |

Holstered no change (15.7 ms in all). After it the Synchronize -> SceneDrv gap is 0.17 ms/frame and the scope window
11.7 ms, of which 3.9 ms are gaps in the scope's own culling stages (Selection, PrepareCollector, DoCulling and the
software occlusion jobs). 25 pause-menu round trips with the Grad drawn, 5 VR menu openings and a save load passed
with 1 / 1: the chain submitted all 284 397 points and 13 231 deferred presents, none timed out. Both are now on by
default (`xr_submit_chain_rev`, src/Core/LiveControlsPoll.cpp: an ini without it gets 1 / 1 once).

Not done: the scope view reusing MAIN's culling. The scope renders before MAIN, so it would get MAIN's previous
frame (the kind of lend the cascades were reverted for).

## The scope's graph was one chain (2026-11-01)

The scope view's nodes did not run serially because of their passes: the engine built the scope's whole render
graph as one chain. `tools/hitch_tests/graph_shape.py` (probe `src/Stereo/RenderGraphShape.cpp`, game started with
`CVR_GRAPH_SHAPE=1`) reads every graph `sub_1403E8F58` runs: MAIN 87 nodes, 443 dependency edges, longest chain 23,
widest level 23; the scope 63 nodes, 169 edges, longest chain 63, widest level 1. Its node list is MAIN's minus six
scene passes plus one (the RTT colour extraction).

The scene graph builder `sub_142276E60(owner, graph, builder, features, data)` decides it at 0x14227B6FF: with
feature bit 64 (f1 bit 0, the render-to-texture extraction bit) it calls `sub_1408EA568`, which makes every node
wait for the one before it, and skips its dependency rules. Those rules cover the RTT view's extra node (the
extraction pass, `sub_1408EAF74`, has its own edges at 0x14227BCB1). `xr_scope_graph_dag` (default 1,
`src/Stereo/ScopeGraphDag.cpp`) makes that one call answer no for the scope's view: a 5-byte call-site patch to a
stub that compares the view's camera name (`[features-2588h]`) and touches only rax and the flags. The scope's graph
is then 82 nodes, longest chain 24, widest level 24.

Megabuilding H10 (ManualSave-491), the Grad drawn and its sight at the right eye (`scope_eye_lh.py`, same pose),
fresh launch per setting, frame period from the Present hook's counters:

| | holstered | Grad at the eye | scope view window | scope node work |
|---|---|---|---|---|
| one chain (engine) | 16.7 ms | 26.2 ms (38 FPS) | 15.0 ms | 11.1 ms |
| xr_scope_graph_dag=1 | 15.9-17.0 ms | 18.5-19.0 ms (53-54 FPS) | 4.9 ms | 3.1 ms |

The scope's cost over holstered went from ~9.5 ms to ~2.6 ms. The GPU is ~93-94 % busy with the scope drawn (was
~80 %), and the scope view is ~13 % of the GPU's scene time (~2 ms a frame, `scopeprof.py gpu`): what is left is the
scope image's own GPU work at 1024 px with MAIN's feature set. The lens image is the same (side-by-side crops, eight
consecutive captures: lens brightness 41.8-42.0, no flicker). Soak with the switch on: 25 pause-menu round trips and
5 VR menu openings with the Grad drawn, five scoped-rifle switches (each builds the scope's graph again), a save load
with the Grad drawn and the rifle drawn again after it.

`xr_scope_gbuffer_partitions=1` on top: the scope's window 3.96 ms against 4.88, the frame the same (18.50 against
18.48 ms); it stays 0.

What does not work:

- Forcing SceneDrv's parallel bit (`work_context[0x30] & 2`) on the scope's serial SceneDrv calls: those are
  half of every view's SceneDrv calls, MAIN's too (39 of 78, the scope 33 of 66). Their children need the state the
  serial path sets up; a scheduled range crashed at once in `exe+0x42692C`.
- A C++ detour of `sub_1403C1C00` (the feature-bit test, a 27-byte leaf): its callers keep values in volatile
  registers across the call (0x1405C4B67 keeps r9), so any compiled detour corrupts them (crash in `exe+0x32AFCC`
  during the load). Hence the call-site stub.
