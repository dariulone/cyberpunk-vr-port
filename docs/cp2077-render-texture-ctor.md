---
name: cp2077-render-texture-ctor
description: constructor of GPU render-texture class 0x2B1BAA0 = sub_1412A3CC0 (parameterless Create); + full primitive set to build option-C target without depot
metadata:
  type: project
---

GPU render-texture object (class vtable RVA 0x2B1BAA0, the bound DynamicTexture handle from
[[cp2077-dyntex-live-recipe]]) is constructed by **sub_1412A3CC0** (RVA 0x12A3CC0), a parameterless
Create(): allocates via sub_14022413C, then:
- *obj = off_142B1BAA0 (vtable)
- u32@0x40 = 256 (width default), u32@0x44 = 256 (height default)  <- set to eye-res before GPU-back
- u32@0x54 = 0x10006 (format/flags); u32@0x64 = 0x01000022 (both verified == live capture)
- ptr@0x98 = off_143009280 (sub-descriptor/generator)
Returns the object in rax. Self-contained CPU descriptor; NO GPU alloc / depot register here.

**Full primitive set for option-C target (NO depot resref needed):**
1. obj = sub_1412A3CC0(); set obj+0x40/+0x44 = eyeW/eyeH.
2. GPU-back: texMgr->vtable[80] = sub_14291A4D4 (ResizeDynamicTexture); texMgr =
   *(*(qword_143427C00)+0x70) (verified live, [[cp2077-render-texmgr-handle]]). Signature
   (rcx=texMgr, rdx=&objHolder, r8d=w, r9d=h, [rsp+20]=flag) — submits a render-thread command.
3. Wire to RTT-camera: write our obj into comp+0x1E8 (resolved handle) directly. Auto-loader
   sub_1423E7768 gates on `!comp[0x1E8] && comp[0x1E0]`, so a pre-set 0x1E8 skips the depot path.
4. sub_1412BAACC(comp, fmt) submits the texture + registers the camera (sub_1423E4658) -> renderer
   draws the camera into our texture each frame.
Next: smoke-test steps 1+2 (create + GPU-back unique-size tex), confirm via RT-diag a same-size RT
cluster appears. Risk: first ACTIVE render-internal calls; keep obj alive (static) to avoid the
orphan-destructor AV seen with CreateInstance.
