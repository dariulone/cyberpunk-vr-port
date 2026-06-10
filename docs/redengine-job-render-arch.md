---
name: redengine-job-render-arch
description: CP2077 render is job-graph based (redJobs2), not a monolithic render-frame — kills UEVR-style Native Stereo in-place; per-view job chain identified in IDA
metadata:
  type: project
---

Runtime stack capture (raw-stack-scan fallback in `DumpRenderCallStack`, dxgi_proxy.cpp — unwind-based RtlCaptureStackBackTrace died at the detour trampoline, so we scan stack memory for in-module return addrs right after a CALL) on the camera locate/patch hooks proved the render path on CP2077 (Steam, ~year-old build, exe base 0x7FF61F3B0000, IDA base 0x140000000).

**Architecture (confirmed from IDA pseudocode):**
- `0x141A3ABF4` = CRT thread bootstrap stub (`unknown_libname_177`, `_acrt_getptd`/`endthreadex`) → camera runs on a **worker thread**, not main loop.
- `0x14016D50E` = `sub_14016D4AC` = thread-proc body (SSE flags, CoInitializeEx, then `vtable[8](a1)`).
- `0x14014324C` = `sub_140143060` = **redJobs2 job-worker dispatch loop**: `while(1)` pulling jobs from lock-free ring queues (@1024/@768, InterlockedCompareExchange64) with work-stealing (`sub_14014379C`), executes job fn ptr from descriptor. String `redJobs2\src\jobScopeMemoryAllocator.cpp`.

**Consequence:** there is NO monolithic render-frame function to call twice → UEVR-style Native Stereo / Synchronized Sequential does NOT port in-place. The frame is a **job graph**. Supersedes the old "AER architectural wall" framing — the wall is the job system, not the dxgi proxy.

**Per-view job chain (job-loop → camera locate `sub_140127F58`):**
`140143060(job loop) → 140243838 → 1402439F9 → 140126D90 → 1404E3C57 → 1401275FB → 140127E66 → 140127F58(locate)`
Functions around `0x243838` = body of the **render-view job**. Real Sync Sequential lead: if that job builds ONE view, make it run twice/frame with per-eye params (vs current alternate-eye stereo-shift injected into a single camera in `OnPatchCameraCallback`).

Shared outer tail wraps BOTH locate and patch in one frame: `14014324C → 14016D50E → 141A3ABF4`.

Decompiled: `0x140243838`=`sub_1402437B0` (generic job-scope wrapper, real body `sub_140243980` via fn-ptr — data-driven, can't climb statically). `0x140126D90`=`sub_140126D4C` = linear camera-system per-frame update (sequence sub_140127B88/AB8/52C/404/01C → locate).

**Runtime ViewProbe (locate rbx = a2 view-descriptor dest), in-world, save loaded OK:** 200 calls over 177 frames = locate fires ~1×/frame. 195/200 hit ONE descriptor `0x21F40F40010`; 3 other rare slots in same object cluster (0x...50/60/540 = secondary cams). **Conclusion: the main camera = a SINGLE view descriptor per frame, NOT a multi-view family.** Reflections/cubemaps use other paths. quat/fov in probe were garbage (hook is ON the fov-write instruction, pre-write).

**Strategic conclusion for the user's goal (Native Stereo / Sync Sequential like UEVR):**
- UEVR Native Stereo does NOT port: Unreal exposes a view family for free; RED4 has one main view/frame + job-graph render. Duplicating the main view in the graph = very invasive.
- **Recommended path = REAL Synchronized Sequential:** keep alternate-eye (1 render/Present) but FREEZE simulation/animation/time between the eye-0 and eye-1 renders so both eyes sample the same sim state, then submit as a synced OpenXR pair. Fixes judder/ghosting (the "фигня" = L/R from different sim frames P and P+1) and fits the single-view architecture. Current `xrSyncSequential` only locks poses (cosmetic stub).
- Next investigation: find where the engine advances sim/frame-time per frame, to gate it so eye-1 reuses eye-0's frame state.

**Native Stereo feasibility dig (IDA):** `sub_140127DE0` = push_back into a DynArray of 160-byte view descriptors (base@*a1, cap@+8, count@+12; grow=sub_1401FFDEC, realloc=sub_140127F24, elem-copy=sub_1425A2FF8). Its ONLY caller is `sub_14012752C`. `sub_140127F58`(locate)=virtual method (9 vtable refs in .rdata, not code xrefs). BUT `sub_14012752C` turned out to be **camera-LAYER BLENDING, not the render view list**: loops over `*(BYTE*)(a1+84)` camera layers, pushes each into a LOCAL temp array (v31, vtable off_142AC0750), weight-blends them (weight@desc+144) into ONE final camera at a1+1216, then destroys the array (sub_140127A98). So RED4 resolves to ONE main game camera/frame. The accessible camera level does NOT expose a render view-family to extend. Render-side multi-view (reflections/eyes) is deeper inside the job-graph renderer (consumer of a1+1216) — that's the true Native Stereo injection point but job-graph-internal = high risk/effort. Traced the blended camera (a1+1216) all the way to render: `sub_140126D4C` calls `sub_140127404` after the blend → `sub_1401274A4(a1, v6)` builds a 904-byte render-camera struct → `sub_14028D4B8(a1+96, v2)` = plain 904-byte struct copy into ONE slot `camera_system+96` (current render camera; not a list append). So at EVERY accessible level (layer-blend → handoff → render-camera slot) RED4 carries ONE main camera; the renderer reads the single `camera_system+96`. **FINAL VERDICT: UEVR-style Native Stereo (append view to a view-family) has NO accessible equivalent in RED4. Multi-view (reflections/shadows) is generated INSIDE the job-graph renderer, data-driven, statically unreachable. Doing two eyes there = forcing the render job to re-draw the main view twice = a harder/riskier version of the same 2nd pass that Synchronized Sequential gives. No cheap win. → Synchronized Sequential is THE path.**

Intermittent crash at menu/save-load (no RED4 dump, no DEVICE_REMOVED = CPU access violation). Added a VectoredExceptionHandler (`VrCrashHandler` in dxgi_proxy.cpp DllMain) that logs faulting IDA RVA + stack scan on next crash — needs build+deploy. See [[vrik-bone-hook-pivot]] for the red4ext side.
