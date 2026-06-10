---
name: cp2077-rendertex-init-driver-stack
description: Watchpoint on render-texture +0xB0 caught init-driver call chain (puppet-preview path our script skips)
metadata:
  type: project
---

True Stereo R2 — HW write-watchpoint on a GAME-created render-texture (class 0x2B1BAA0) obj+0xB0,
armed via INT3 on ctor sub_1412A3CC0 during inventory puppet-preview.

First non-zero write to +0xB0 landed = obj+0x90 (interior self-pointer => embedded DynArray/container
storage init, NOT a GPU pointer). rip = 0x141908809 (generic memset_repmovs leaf). So +0xB0 is an
embedded container head, churned by CPU init — likely a red herring for GPU backing.

VALUE: the captured caller stack IS the render-texture INIT DRIVER the game runs during puppet-preview
(and which our script AddComponent path never triggers). Chain (outer->inner):
  0x14012E437 -> 0x14012F1AE -> 0x14012F394 -> [vtable 0x142B31928] -> 0x14012F3EA
   -> 0x1401CBA23 -> 0x1401CAF3C -> 0x14014BABD(dynamicBuffer memset)
Cluster 0x12Exxx/0x12Fxxx = render-texture own init methods. Next: decompile 0x14012E437 / 0x14012F1AE
to find the callable init/GPU-back driver. See [[cp2077-gpu-backing-allocator-found]] [[cp2077-rendertex-needs-generator]].
