---
name: cp2077-view-registry-live
description: live-found view/pass registry object (vtable 0x2B57F58) keyed by pass-id, entries reference the single main camera
metadata:
  type: project
---

Live RPM (game open) found the **view/pass registry** for the main render frame: object `vtable=exe+0x2B57F58` (one live instance at 0x242289C5550 this session — heap addr is per-session).

- Found by scanning heap for pointers to the live main-view camera (vtable `exe+0x2B72EE8`, FOV 95). ~40 entries reference the SAME camera, keyed `0x10000..0x1002C` (+ `0xFFFF` sentinels), all sharing back-ptr = the registry object. The ~40 count == the **40 rtId passes** the SceneDrv hook saw (80 passes = 40 × 2/frame).
- Entry layout (~0x40): `+0x00 key(0x10000+idx)` / `+0x08 back-ptr(registry)` / `+0x10 camera` / `+0x18 obj` / `+0x20 hash` / `+0x30 scaleXY (1.0,1.0 floats)`.
- So one main camera fans out into ~40 keyed render passes via this registry; FrameGraph build (sub_140AA3904, once/frame) iterates it. See [[cp2077-framegraph-build-chain]].

Implication for simultaneous stereo: a 2nd eye = a 2nd full pass-set. Cheapest untested lever = **2nd FG-build per frame** with swapped global view (`renderer+0x4658`); FG-build auto-allocates fresh RTs per build, dodging the OOB/no-op walls that killed consumer-level replay ([[cp2077-node-replay-not-viable]], inline view re-invoke was a no-op). FG-build hook already has clean call-original thunk `g_fgBuildCallOriginal`. Camera pose for right eye = AER LocateCamera mechanism (consumer-side, below render-view task).
