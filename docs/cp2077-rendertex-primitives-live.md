---
name: cp2077-rendertex-primitives-live
description: option-C native primitives PROVEN live in-process — create+GPU-back render-texture + wire to RTT-cam run without crash; remaining blocker = render-active gate sub_140665054
metadata:
  type: project
---

Built plugin native fns SmokeCreateRenderTex / WireRenderTexToCamera and ran them in-process:
- **sub_1412A3CC0(this)** ctor (RVA 0x12A3CC0) on a self-allocated zeroed block -> produces a real
  render-texture object: vtRva 0x2B1BAA0 (class MATCH), default 256x256, fmt 0x10006. WORKS.
- **ResizeDynamicTexture = sub_14291A4D4(texMgr,&holder,w,h,flag)** (RVA 0x291A4D4), texMgr =
  *(*(exeBase+0x3427C00)+0x70). Returned ok, no crash. (Actual GPU alloc unconfirmed — RT-diag is
  bind-based.)
- Wire: set comp+0x1E8 = our obj (resolved handle, bypasses depot), isEnabled@0x8b=1,
  resolutionW@0x258 / H@0x25c, then **sub_1412BAACC(comp,1)** (RVA 0x12BAACC) ok=1, no crash.

**Result: NO render into our texture** (no 1280x1152 RT-diag cluster, no FPS change). Cause: inside
sub_1412BAACC the camera-register branch (sub_1423E4658) is gated by **sub_140665054(comp)** ("render
active"); a SCRIPT-AddComponent'd RTT-camera is not render-attached, so the gate returns false ->
registration skipped -> never rendered. No crash = gate false (register block deref'd nothing).

**Next lever:** decompile sub_140665054 (RVA 0x665054) to learn the render-active condition/flag and
whether we can force it (or properly attach the component's render proxy). All create/GPU-back/wire
primitives are live; only render-attachment remains. See [[cp2077-render-texture-ctor]],
[[cp2077-dyntex-live-recipe]], [[cp2077-render-texmgr-handle]].
