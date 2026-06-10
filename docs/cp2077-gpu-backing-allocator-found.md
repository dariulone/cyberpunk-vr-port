---
name: cp2077-gpu-backing-allocator-found
description: HW watchpoint caught the +0xB0 GPU-backing allocator instruction at ida=0x141908809
metadata:
  type: project
---

Native True Stereo crux (GPU-backing allocation for DynamicTexture) LOCALIZED via INT3+HW-watchpoint capture.

Method: INT3 one-shot on render-texture ctor sub_1412A3CC0 (RVA 0x12A3CC0) caught rcx=fresh-texture-obj; armed DR HW write-bp on obj+0xB0 across 168 threads; opening inventory (puppet-preview) triggered the write.

RESULT: the instruction that writes GPU backing into render-texture +0xB0 is at **ida=0x141908809 (rva=0x1908809)**. This is the allocator we could not reach statically (it lives inside the async generator/CRenderNode_GenerateAsyncDynamicTextures path, ~7KB function sub_141D43040 region; the actual write is sub_1419xxxxx).

Next: decompile the function containing 0x141908809 -> understand GPU-backing alloc + what registration/generator-setup makes the texture visible to the async render-node. This is the final missing primitive for native True Stereo (create+GPU-back our own eye-RT from plugin code).

Related: [[cp2077-rendertex-needs-generator]] [[cp2077-rendertex-primitives-live]] [[cp2077-render-texmgr-handle]] [[cp2077-dyntex-live-recipe]]