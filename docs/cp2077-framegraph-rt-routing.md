---
name: cp2077-framegraph-rt-routing
description: D3D12 RT-capture verdict — scene renders to offscreen pooled 3072² HDR RTs; per-view RT routing is automatic via FrameGraph transient pool; lever for C is a distinct view-object, not manual RT management
metadata:
  type: project
---

CP2077 RT-routing for option C, observed directly at the D3D12 boundary (hooked
CreateRenderTargetView slot 20 + OMSetRenderTargets slot 46 in dxgi_factory_wrapper —
stable infra, same as depth-capture). One MONO capture at a working mirror.

**Findings:**
- Main scene-color = **offscreen RT 3072×3072** (per-eye render res), a whole SET of
  3072² targets in formats 24(R10G10B10A2)/26(R11G11B10F)/28/10/34/49/62 = GBuffer MRT,
  lighting, post. Scene renders to offscreen color, **NOT the swapchain backbuffer**.
- **Thousands of distinct resources** per size class → RTs are **transient, pool-allocated
  by FrameGraph each frame** (the lazily-filled resource table at qword_143438A28+0x2F1D8).
- Odd sizes (3071×2700, 2962×2786…) are the main view under **dynamic resolution scaling**,
  not the mirror. Mirror RT is among smaller classes (1536×1536 / 1401×1353).

**Verdict (key):** per-view RT-routing is **AUTOMATIC** — FrameGraph allocates each view's
transient RTs from a pool (that's how the mirror gets its own texture with no manual bind).
=> We do NOT need to reverse the 27.8MB FrameGraph instance or hand-manage the resource
table. The lever for C is **getting the engine to render a distinct view-object** (like the
mirror); the graph auto-allocates its RT. Then grab that view's final color RT, submit as
right eye. Confirms the long-standing conclusion that C needs a distinct view-object
(see [[cp2077-true-stereo-mirror-mechanism]] / [[cp2077-perview-camera-layout]]).

Eye-RT to match if ever created manually: 3072×3072, fmt 24 or 26 (HDR) for scene,
28/29 for final. Code-patch view hooks (ViewSchedule/Iterator/TaskDispatch/CamUpdate) were
DISABLED for this run (kEnableOptionCCodeHooks=false in dxgi_proxy.cpp) — load was stable.
