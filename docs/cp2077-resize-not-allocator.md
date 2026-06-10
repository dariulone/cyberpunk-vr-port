---
name: cp2077-resize-not-allocator
description: descriptor-swap test NEGATIVE - ResizeDynamicTexture is not the GPU allocator; backing comes from generator/resource path
metadata:
  type: project
---

CP2077 native True Stereo (option-C) crux test, 2026-06-08. Hypothesis that ResizeDynamicTexture
(texMgr vtable[80] = sub_14291A4D4) allocates GPU backing once obj+0x98 holds the render-capable
descriptor (off_1431A9280) is **DISPROVEN**.

Smoke test: ctor sub_1412A3CC0 -> set obj+0x98 = exeBase+0x31A9280 (render descriptor) -> set
res 1280x1152 -> ResizeDynamicTexture(texMgr,&obj,w,h,1). Result: returns ok=1 but **obj+0xB0
(GPU sub-struct) stays NULL**. Earlier WireRenderTexToCamera+wait-5s also showed +0xB0 null.

Conclusion: ResizeDynamicTexture only resizes an ALREADY GPU-backed texture; it is NOT the
allocator. GPU backing (+0xB0) + shared-obj (+0x10) + render descriptor (+0x98=off_1431A9280)
are all produced by the **IDynamicTextureGenerator / resource-node path** (CRenderNode_
GenerateAsyncDynamicTextures / PrepareDynamicTextures), which our manual ctor+resize bypasses.

Next native lever (unproven): capture the engine's real create path by hooking render-texture
ctor sub_1412A3CC0 during a live puppet-preview, log the return address = the generator/resource
allocator that fills +0xB0. See [[cp2077-rtt-instance-vtable]] [[cp2077-render-texmgr-handle]]
[[cp2077-dyntex-live-recipe]].
