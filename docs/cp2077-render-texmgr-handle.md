---
name: cp2077-render-texmgr-handle
description: CP2077 render texture-manager singleton handle = *(renderer+0x70); ResizeDynamicTexture is its thread-marshaled vtable method (GPU DynamicTexture alloc)
metadata:
  type: project
---

True-Stereo R2 — last handle found. Render init `sub_140613584(renderer,...)` builds the renderer sub-system table. `a1`==renderer base == global `qword_143427C00` (proven: this fn uses `a1+0x4658`, the render-view matrices slot from [[redengine-render-view-state]]).

**Texture-manager** (vtable `off_142C11958`, RVA 0x2C11958) is constructed by `sub_141049E14`→`sub_141049E6C` and stored at **`renderer+0x70` (112)**.

Handle path (in-process, read-only verifiable): `renderer_base = *(qword_143427C00)` [RVA 0x3427C00]; `texMgr = *(renderer_base + 0x70)`; verify `*texMgr == off_142C11958` (RVA 0x2C11958).

`ResizeDynamicTexture` = `sub_14291A4D4` = texMgr vtable method (dq @ .rdata 0x142C11BD8). Does NOT call directly — builds a render command (`off_1431D8548` vtable) and submits to render-thread via `sub_142910480` → **thread-marshaled** GPU alloc.

Sibling sub-system slots: renderer+96/104/112/120/128/152/160/216/224... renderSys for camera-register (sub_1423E4658, see [[cp2077-rtt-no-drivable-api]]) is likely one of these.

VERIFIED LIVE (VerifyTexMgrHandle plugin fn, CET-callable, returns 1): in-world, `*(*(qword_143427C00)+0x70)` vtable RVA == 0x2C11958 confirmed. Sibling slot `renderer+0xE0` has vtable RVA 0x2AFB688 (renderSys candidate); other slots store handle/data wrappers (no module vtable).

texMgr is a GENERIC render-command PROXY: each vtable method builds one named render command (e.g. ResizeDynamicTexture[80], UpdateLightFlickerParams[79], UnloadMorphData[78], SuspendParticles[73], ResumeParticles[74]) and ENQUEUES it to the render-thread (sub_14291xxxx submit) — so calling from game/CET thread is thread-safe (marshaling built-in). No standalone "CreateDynamicTexture" command among Resize neighbors → DynamicTexture creation is game-side (CreateInstance + resource-load); ResizeDynamicTexture only allocates GPU backing of an EXISTING render-texture object (resize 0->WxH = alloc).

CAVEAT re-read of `sub_14291A4D4` disasm: it does `rbx=*rdx; if(!rbx) skip; lock inc [rbx+8]` then builds resize cmd → it **resizes an EXISTING texture object**, not create-from-nothing. So a separate CREATE/alloc method (same texMgr vtable, off_142C11958) is still needed to make the GPU-backed DynamicTexture object.

DynamicTexture layout (DumpDynTexLayout plugin fn): fresh CreateInstance = pure DATA SHELL — width@0x40,height@0x44,dataFormat@0x54,scaleToViewport@0x48,mipChain@0x55,samplesCount@0x56,path@0x30, **generator@0x88 (handle:IDynamicTextureGenerator)=null**. NO render-texture sub-object. So render-backing is produced by the GENERATOR, not by texMgr.ResizeDynamicTexture (which has nothing to resize). Crux = the generator pipeline (camera→IDynamicTextureGenerator→DynamicTexture→render-node PrepareDynamicTextures), not a single texMgr call. WARNING: CreateInstance(true) of DynamicTexture and orphaning it CRASHES on finalization (destructor touches render state) — don't freely CreateInstance render-resource types.

Remaining for native True Stereo: (1) verify handle read-only [DONE r=1], (2) call ResizeDynamicTexture thread-correctly to GPU-back a DynamicTexture, (3) register right-eye camera, (4) grab+submit+pose. Links [[cp2077-rtt-instance-vtable]].
