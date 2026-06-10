---
name: cp2077-d3d12-replay-feasibility
description: "D3D12 command-replay true-stereo (Gemini idea) feasibility — decisive numbers from ALREADY-DEPLOYED hooks (no rebuild): view-dependent = 72-80 passes / 35-40 distinct RTs / 4 queues / ~133k ECL calls/snapshot; camera = decomposed matrix state block (not a single 4x4 CBV). RPM can't answer this (dynamic stream)."
metadata: 
  node_type: memory
  type: project
  originSessionId: a4faea66-21d0-4884-a9b4-10c3d5515440
---

User asked (2026-06-09) to test the Gemini-proposed D3D12 path (hook the command stream; render the recorded view-dependent passes twice with a swapped camera CBV + redirected output = true stereo below the engine) and asked if a live memory read (RPM) suffices.

**Honest tool note:** RPM (tools/rpm.ps1, ReadProcessMemory) reads STATIC process memory (camera structs, render-view state @renderer+0x4658, view objects). The D3D12-replay feasibility is about the DYNAMIC command stream (how often the camera CBV is bound, which RTs each pass writes, command-list re-executability) — RPM cannot see that. The DEPLOYED dxgi.dll ALREADY hooks the relevant D3D12 calls (ExecuteCommandLists/ECL-DIAG, OMSetRenderTargets, CreateDepthStencilView slot21, CreateRenderTargetView slot20, DLSS camera-matrix) and already logs them, so the decisive data was already captured — no new build needed.

**Decisive numbers (from cyberpunkvrport.log, live, RT off):**
- View-dependent scene portion (SceneDrv sub_1401EC1D0, per frame): **72-80 passes writing 35-40 DISTINCT render-target IDs** (`[SCENE-DRV] totalPasses=72..80 distinctRtIds=35..40`). This is the RT set a right-eye replay must NOT overwrite = the clone/redirect count.
- Command volume: ~**133k ExecuteCommandLists calls** / ~352k command-lists per ECL-DIAG snapshot, **94.7% on the present queue, 4 distinct queues**. Huge multi-queue graph.
- Camera: DLSS hook finds a camera matrix STATE BLOCK but "no projection-like 4x4" => matrices stored DECOMPOSED (quat/fov/nearfar per [[cp2077-perview-camera-layout]]), not a single bindable 4x4 CBV. So "swap one CVV" is not literally one buffer; the GPU CBVs are filled from this block per-pass.

**Verdict on the Gemini approach:**
- Naive form (clone all RTs + rewrite every pass's bindings) = manually re-implementing the FrameGraph's per-view RT routing for 35-40 RTs across 72-80 passes / 4 queues. Enormous + fragile (descriptor heaps/bindless, root sigs, resource-state barriers => DEVICE_REMOVED risk, already bitten).
- GENUINE NEW ANGLE vs the dead native levers: at D3D12 I allocate my OWN RTs (CreateCommittedResource) — this BYPASSES the engine view-registration / arena-slot wall ([[cp2077-clone-crash-global-arena-slot]]) that killed every native lever. So it's "lots of mechanical work", not "architecturally impossible".
- TRACTABLE variant (sequential replay): don't clone all 35-40 RTs — let eye-1 finish + capture its final color (we already do, CapturePresentedFrame), then swap the camera block to right-eye and RE-EXECUTE the recorded view-dependent command lists (intermediates safely overwritten since eye-1 is done), redirecting only the FINAL color to a right-eye RT. This = "Synchronized Sequential implemented at the D3D12 command-replay level." Open risks: resource-state/barrier reset between executions; whether recorded command lists are re-executable (D3D12 allows it, but CP2077 re-records per frame); stable upload-heap location of the camera block to overwrite.
- BUT the tractable variant converges on the SAME experiential result as engine-level Synchronized Sequential ([[redengine-job-render-arch]]) at higher complexity/fragility and no clear win — UNLESS we want FULL-RATE simultaneous (both eyes every frame), which forces the heavy 35-40 RT clone path.

**Bottom line:** D3D12 replay is the only category NOT walled architecturally (own RTs sidestep arena registration), but the captured numbers show full-rate-simultaneous = clone+rebind 35-40 RTs / 72-80 passes (very heavy), and the cheap variant = Sync-Sequential-at-D3D12 (no advantage over engine-level Sync Sequential). Next cheap decisive probe if pursued: identify the camera constant upload-heap address + test re-executing the captured per-frame command lists once with swapped constants without DEVICE_REMOVED.
