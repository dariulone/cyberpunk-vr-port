---
name: cp2077-rendertex-needs-generator
description: option-C shortcut insufficient — ctor+ResizeDynamicTexture produces a bare CPU descriptor; live DynamicTexture handle is fully populated by the generator/resource-load path (+0x98 swapped, +0xB0 GPU sub-struct)
metadata:
  type: project
---

DumpRenderTexState on our created render-texture (class 0x2B1BAA0) AFTER ctor+ResizeDynamicTexture,
compared to the LIVE captured puppet-preview handle ([[cp2077-dyntex-live-recipe]]):

| field | ours (ctor+resize) | live handle |
| +0x00 vtable | 0x2B1BAA0 ok | 0x2B1BAA0 |
| +0x40 res | set ok | set |
| +0x08 refcount | 0 | self-ref nonzero |
| +0x10 shared sub-obj | 0 | populated |
| +0x98 descriptor | 0x3009280 (ctor default) | **0x31A9280 (swapped)** |
| +0xB0 GPU sub-struct | 0 | heap obj |

**Conclusion:** ResizeDynamicTexture (sub_14291A4D4) does NOT create GPU backing — it resizes an
ALREADY-backed texture (its `if(!rbx) skip`/refcount path operates on an existing resource). The bare
ctor output is a CPU descriptor only. The LIVE handle's GPU backing + generator wiring (+0x98 swap to
0x31A9280, +0xB0 sub-struct, +0x08/+0x10) are produced by the **generator / resource-load path** we
bypassed. So the "skip the depot, inject comp+0x1E8 directly" shortcut yields an incomplete texture
-> camera wires + registers (gate sub_140665054 passes because we set 0x1E8) but renders nothing
(no 1280x1152 RT-diag cluster, refcount back to 0).

**Real remaining crux:** reproduce the generator+GPU-alloc setup. RTT-camera has
`generator: IDynamicTextureGenerator @0x88`; the texture's +0x98 must point to a real generator
(live=0x31A9280) and the render node CRenderNode_PrepareDynamicTextures / GenerateAsyncDynamicTextures
allocates GPU backing from it. This is the resource/generator subsystem — deep. Alternatives: proper
depot registration (no clean API found), or reverse what sets +0x98/+0xB0 in the live path.
All callable primitives proven (no crash); the gap is the generator/GPU-alloc, not the call ABI.
