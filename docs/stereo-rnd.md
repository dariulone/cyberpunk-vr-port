# Genuine Stereo R&D — findings & dead ends

This document records an extended research effort to replace **AER (Alternate Eye
Rendering)** with *genuine simultaneous stereo* in Cyberpunk 2077 (REDengine 4),
so that both eyes come from one engine frame and the AER head‑pose ghosting
disappears.

**Outcome: not shipped.** Every avenue that produces a *correct* image was either
architecturally walled or required per‑pass surgery far beyond a normal patch.
The working tree from this effort is preserved as a reversible patch
(`docs/stereo-rnd-experiment.patch`, `docs/stereo-rnd-diffstat.txt`) and the
shipped code was reverted to the stable **AER** baseline. This file explains
*why*, so the next attempt doesn't re‑walk the same dead ends.

---

## TL;DR

| Approach | Result |
| --- | --- |
| D3D12 **in‑place command replay** (render the frame's scene batches twice) | **HANG** — scene batches use FrameGraph transient/aliased memory braided with async compute; re‑executing them in any order/timing wedges the async queue. |
| D3D12 **end‑of‑frame replay** (re‑submit captured batches after Present) | **GPU CRASH** (device‑removed) — recorded scene lists reference transient resources whose heap memory + descriptors are reassigned by frame end. |
| **Shader‑injection** stereo (geo‑11 / 3Dmigoto style: one engine pass → both eyes via an injected geometry shader) | **Geometry works, image unusable** — CP2077 is a *deferred* renderer; the GS only controls G‑buffer geometry. Lighting / shadows / SSR / SSAO / volumetrics / post / RT / DLSS are mono screen‑space passes the GS can't touch, so side‑by‑side packing yields an "intersection of two pictures". |

**The two realistic paths forward** (neither attempted here) are listed at the end:
**Synchronized Sequential** submit, or polishing the existing **AER** path
(optical‑flow / reprojection) to minimize ghosting.

---

## 1. Why genuine simultaneous is hard in REDengine

REDengine 4 renders each frame as an **asynchronous job graph** (`redJobs2`),
not a synchronous "draw the scene now" call. There is **one** main view per
frame, view‑independent and per‑eye passes are interleaved, and the render
functions are pointer‑dispatched (statically unreachable). Across many prior
sessions every *engine‑level* lever to register a second simultaneous view was
exhausted (producer double‑call tears shared per‑frame manager state; the
view‑family arena assigns one slot; the RTT‑camera path consumes a handle it
never allocates, etc.).

That left two **D3D12‑level** ideas — replay and shader injection — both pursued
here.

## 2. D3D12 command replay (DEAD)

Idea: hook `ID3D12CommandQueue::ExecuteCommandLists`, capture the frame's
present‑queue batches, and run the view‑dependent scene batches a second time
with a shifted camera = a second eye *below* the engine.

Instrumentation (`dxgi_factory_wrapper.cpp`) added a per‑batch double‑submit
probe with barrier‑state compensation, a detached watchdog that names the exact
stuck batch via fence markers, an `ExecuteIndirect` census, and a file‑driven
ordinal mask (`cyberpunkvrport_m0.txt`) for bisection without rebuilds.

Findings:

- **Non‑transient batches re‑execute for free** — frame‑begin/end, a few prep
  batches (ordinals `{0,1,2,10,13}` in the test scene) doubled for minutes with
  **0 ms** added frame time. Encouraging, but they contain **no scene**.
- **All scene batches use FrameGraph transient / aliased memory**, braided with
  async compute (lighting/AO/GI on a separate queue, sharing the transient
  pool). Re‑executing a scene batch *in‑place* → the duplicate stomps memory the
  async queue is still reading → GPU stall → the async fence never signals →
  the present queue's `Wait` blocks forever (engine watchdog kills the process
  at 120 s).
- **End‑of‑frame replay** (re‑submit captured lists after `Present`) → instant
  **GPU crash** (device‑removed), because the recorded lists reference transient
  resources whose heap + descriptors were already reassigned by frame end.
- **RTV/DSV are baked into the command list at record time** (only shader‑visible
  CBV/SRV/UAV are read at execute), so you can't even redirect a recorded scene
  pass into a per‑eye clone target.

Conclusion: the engine's scene command lists are bound to the exact
frame‑execution context (live transient + coordinated async) and are **not
re‑executable** at any timing. A genuine second render must be **recorded by the
engine** — i.e. engine‑level, which is exhausted.

## 3. Shader‑injection stereo (geometry works, image unusable)

Idea (the geo‑11 / 3Dmigoto technique): don't replay the frame — make the
engine's *single* pass emit **both eyes** by injecting a geometry shader that
duplicates each triangle with a per‑eye clip‑space offset, packed side‑by‑side.

What worked (gates 1–4a, all passed live):

1. **Runtime SM6 GS compilation in‑process** — bundled `dxcompiler.dll` +
   `dxil.dll` compile `gs_6_0` inside the game process.
2. **Signature‑matched GS generation** — reflect a real scene VS (`IDxcUtils::
   CreateReflection` on DXIL), generate a pass‑through GS whose I/O exactly
   matches the VS output signature (incl. `SV_Position`, `SV_ClipDistance`).
3. **PSO injection** — copy the `D3D12_GRAPHICS_PIPELINE_STATE_DESC`, insert our
   GS, recreate the PSO; on failure fall back to the original desc so a draw is
   never broken. **1500+ G‑buffer PSOs injected with zero artifacts.** CP2077
   uses the classic `CreateGraphicsPipelineState` (slot 10); stream slot 47 is
   unused; VS/PS are readable `DXBC` containers wrapping **DXIL (SM6)**.
4. **Stereo duplication confirmed live** — GS emits each triangle twice with the
   canonical NVIDIA footer `clip.x += sep*(w − conv)`; objects visibly doubled
   with head‑move parallax.

The wall — **CP2077 is a deferred renderer:**

- The GS controls **only geometry** (G‑buffer fill: position/normal/material).
- The *bulk* of the frame — deferred lighting, shadows, SSR, SSAO, volumetrics,
  fog, post, RT (RTGI/reflections), DLSS — are **screen‑space / compute passes**
  over the whole buffer with a **mono camera**, untouched by the GS.
- Side‑by‑side packing therefore produces an **"intersection of two pictures"**:
  the G‑buffer geometry is packed into halves, but depth‑prepass / forward /
  transparency / lighting render full‑width over it → overlap, see‑through
  walls, doubled HUD.
- The HMD side‑by‑side capture also added a **synchronous `WaitForSingleObject`
  fence wait** on the present queue → **0 FPS** on heavy load frames.

geo‑11 / 3D Vision work cleanly on **forward** renderers (one shader does
geometry *and* lighting per draw), or by the *driver* duplicating **every** draw
into two RTs with per‑eye constant buffers (= full per‑pass duplication = the
replay path proven dead above). Shader‑injection stereo from *outside* a
deferred engine means per‑eye‑ifying **every** screen‑space + temporal pass —
multi‑month, uncertain, and possibly unreachable for RT/DLSS.

## 4. The flag system (for anyone re‑applying the patch)

All experimental behavior is gated behind a text file next to the game exe,
`bin/x64/cyberpunkvrport_m0.txt`, format: `mask flags sep conv packX`
(hex mask, hex flags, three floats). `0x0 0x0` = native AER, untouched.

| flag bit | meaning |
| --- | --- |
| `0x20` | GS injection into G‑buffer PSOs |
| `0x40` | stereo GS (duplicate + per‑eye clip offset / SBS pack) |
| `0x100` | XR side‑by‑side capture (split halves to eyes) |
| `0x200` | debug: whole backbuffer to both eyes |
| `0x400` | experimental 2× ‑wide render (no zoom) |
| `0x800` | debug: emit only the left‑eye copy |
| `0x1000` | strict G‑buffer filter (`numRT>=3`, the Gate‑4a config) |

The matching ImGui "Stereo (SBS)" render‑mode toggle lives in the overlay.

## 5. Files touched (see the patch for the exact diff)

- `src/dxgi_factory_wrapper.cpp` — D3D12 replay probes (M0/M2), watchdog, ordinal
  mask, PSO/shader recon, GS injection + stereo pack, side‑by‑side capture
  plumbing, the flag getters.
- `src/dxgi_proxy.cpp` — flag parsing, 2×‑wide render width, per‑eye IPD‑shift
  suppression under SBS, time‑system / skip‑tick reversing, RTT investigation.
- `src/openxr_manager.{cpp,h}` — `CaptureSideBySide` (stretch‑blit each half to a
  per‑eye image), SBS submit integration, mono fallback.
- `src/imgui_overlay.cpp` — "Stereo (SBS)" render‑mode option + separation slider.
- `src/red4ext_plugin/main.cpp` — `gameTimeSystem` dump / time‑dilation (Skip‑Tick
  experiment, later disproven: freezing sim does not fix AER head‑pose ghosting).

## 6. Honest next steps (not done here)

1. **Synchronized Sequential** — submit both eyes on the *same* engine tick
   (UEVR‑style), removing the inter‑eye latency that causes AER ghosting, without
   a second simultaneous render. The blocker is that CP2077 lacks Unreal's
   `IStereoRendering`; it needs an engine‑tick hook that re‑renders the viewport
   for the second eye within one tick.
2. **Polish AER** — depth‑aware reprojection / optical‑flow (the half‑built
   `AER V2` track) to mask the ghosting AER already exhibits, keeping DLSS + RT
   native per frame.

The shader‑injection foundation (in‑process SM6 compile, signature‑matched GS,
safe PSO injection) is real and novel for DX12/CP2077; it's preserved in the
patch should a future effort want to tackle the per‑pass deferred problem.
