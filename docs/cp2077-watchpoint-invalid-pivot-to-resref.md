---
name: cp2077-watchpoint-invalid-pivot-to-resref
description: +0xB0 watchpoint caught REUSED memory (polygon code) — invalid; real path = give comp+0x1E0 a valid resref, engine auto-backs
metadata:
  type: project
---

CORRECTION to [[cp2077-rendertex-init-driver-stack]]: the +0xB0 watchpoint capture was INVALID.
Decompiling the captured stack (0x14012F190) = shoelace polygon-area math (2D polygon DynArray), NOT
render-texture init. The render-texture object caught at ctor sub_1412A3CC0 was a TRANSIENT — freed and
its memory REUSED for a polygon buffer before the watched write fired. The interior ptr obj+0x90 and
memset rip confirm reuse. The INT3-on-ctor + HW-watchpoint instrument is unreliable here (address reuse
between capture and write).

STRATEGY PIVOT (verified by the wire/auto-loader logic):
- Camera REGISTRATION already works (our wire ok=1; gate sub_140665054 passes on comp+0x1E8).
- Render fails ONLY because comp+0x1E8 texture has no GPU backing.
- GPU backing is produced by the generator/resource-load path, which the engine runs AUTOMATICALLY via
  auto-loader sub_1423E7768: if(!comp[0x1E8] && comp[0x1E0]) { load resref@0x1E0 -> generate -> GPU-back
  -> register }.
- We never tried giving comp+0x1E0 a valid depot RESREF. We stuffed a raw CreateInstance object into
  +0x1E8 (bypassing the generator) => no backing. The live working comp has +0x1E0 = 0xD55FB557EAEBA05B
  (depot resref), +0x1E8 = resolved handle.
=> Correct path: obtain/author a valid render-texture (DynamicTexture) depot resource, put its resref in
   comp+0x1E0, let the engine auto-back+register+render. Stop building the texture bottom-up.
See [[cp2077-dyntex-live-recipe]] [[cp2077-render-texmgr-handle]].
