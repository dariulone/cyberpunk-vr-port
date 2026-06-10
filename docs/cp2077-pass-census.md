---
name: cp2077-pass-census
description: CP2077 frame pass census (Nsight, RT off) — view-independent vs per-eye split, kills the case for true-simultaneous-stereo (option C)
metadata:
  type: project
---

Nsight Graphics frame capture of CP2077 (RT/PathTracing OFF in UserSettings.json — the real VR scenario), exported EventList.csv. Pass names recovered from D3D12 `SetName` on command lists (queue-tagged `[ptr Default]`/`[ptr Compute] PassName`). RenderDoc cannot capture CP2077 — game hard-probes DXR 1.1 device cap at startup and exits with a ray-tracing error even with RT off; use Nsight Graphics or PIX (full DXR).

**View-INDEPENDENT (shareable between eyes), ~3700 cmd-lists ≈ ~12% of frame:**
RenderCascade0/1 (CSM sun shadows, light-POV), RenderDistantShadows, RenderLocalShadows0/1, ClearShadowCascades, AsyncComputeDuringShadowmaps, WindUpdateAndRainMap, AsyncWater, DecoupledParticleLighting, PrepareGloballyAccessibleSystems/RenderPrep/StorageData/FrameTick.

**View-DEPENDENT (must redo per-eye), ~27000 cmd-lists ≈ ~88%:**
GBuffer_Solid(7515), GBufferVelocity_Solid(4606), DeferredDecals(3471), Transparents(2958), DepthPrepass(2024), GBuffer(Velocity)_Discard(2267), HologramDepth_and_Distortion(1057), Lighting(937), Hair_Opaque+Skin_Post_and_Forward(1450), PostFX/TXAA_Blurs/DoF_MotionBlur/PostGBufferOverrides/Forward_NoTXAA(~830).

**Verdict on option C (true simultaneous stereo = re-run FrameGraph twice with shared passes):** ceiling benefit only ~6-12% frame (share shadows → 2nd eye at ~88%, stereo = ~188% vs Sync Sequential 200%). Requires making the non-reentrant FrameGraph (barrier tracker [[redengine-render-view-state]] sub_1401F41F8 is single-pass) re-entrant + surgically split shared/per-eye passes. Not worth it vs a well-done Synchronized Sequential. Counts are cmd-list counts, not GPU ms — confirm exact % with Nsight GPU Trace if needed, but direction is clear: shareable work is a minority. See [[redengine-job-render-arch]].
