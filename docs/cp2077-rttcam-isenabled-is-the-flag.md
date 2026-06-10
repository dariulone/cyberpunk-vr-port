---
name: cp2077-rttcam-isenabled-is-the-flag
description: The attached/detached discriminator comp+0x88 (0x01000003 vs 0x3) is literally the RTTI isEnabled Bool @0x8b; engine attaches/renders RTT cam when isEnabled=1
metadata:
  type: project
---

Decisive correction from RTTI dump (`reflection_rtti_dump.txt`) cross-checked with the live attached-vs-detached diff.

`entRenderToTextureCameraComponent` (size 0xA10, base entBaseCameraComponent) RTTI props that matter:
- `isEnabled : Bool @0x8b`
- `renderingMode : entRenderToTextureMode @0x290` (Shaded=0, GBufferOnly=1)
- `resolutionWidth @0x258`, `resolutionHeight @0x25c`
- `dynamicTextureRes : raRef:DynamicTexture @0x1e0` (hash@0x1E0, resolved handle@0x1E8)
- `features @0x9f6`, `renderSceneLayer @0x9f4`, `params @0x280`

**Key insight:** the "attach flag" in [[cp2077-rttcam-attached-vs-not-live]] / [[cp2077-live-rpm-and-attach-pivot]] — comp+0x88 = `0x01000003` (attached) vs `0x3` (detached) — differs in EXACTLY one byte: offset `0x8b`, which IS the `isEnabled` Bool. So attached == `isEnabled=1`. The inventory driver (`gameuiInventoryPuppetPreviewGameController`, `gameuiPuppetPreviewCameraSetup`, `gameuiPuppetPreview_SetCameraSetupEvent`) sets `isEnabled=1` on open, `0` on close — that is what we watched flip live.

**Why this matters:** the differentiator between our non-rendering script camera and the working preview camera may be just `isEnabled`. Next experiment: set `isEnabled=1` (+ resolution + renderingMode=Shaded) on our script RTT cam and verify via live-RPM whether the engine creates comp+0x1E8 handle. Open question: does flipping the byte suffice, or must it go through the engine's SetEnabled/OnPropertyChanged path to trigger render re-attach (component vmethod, vtable `exe+0x307BFD0`). No RTTI member fn exists for it (propCount=32, 0 member funcs).

The RTT render is driven by UI preview controllers (inventory/photomode/wardrobe `gameui*PreviewGameController`, `entEntityPreview`, `RenderTexturePreviewData`) — confirms [[cp2077-rtt-no-drivable-api]]: no general "render camera to texture" API, it is preview-context driven.
