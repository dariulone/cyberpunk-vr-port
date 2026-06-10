---
name: cp2077-isenabled-necessary-not-sufficient
description: DECISIVE test - setting isEnabled=1 on all 19 idle preview RTT cams made +0x88=0x01000003 but produced NO comp+0x1E8 handle; isEnabled is necessary, not sufficient - entity must be render-active
metadata:
  type: project
---

Test via `Game.ForceEnableAllRTTCams()` (off-thread heap scan by vtable exe+0x307BFD0, enable every cam with +0x88==0x3), then live-RPM re-scan (tools/scan_rttcam.ps1):

Result:
- All 19 valid RTT cams went from `+0x88 = 0x3` to `+0x88 = 0x01000003` (isEnabled byte @0x8b set) — byte-for-byte identical to the WORKING attached preview cam.
- **`comp+0x1E8 (handle) = 0x0` on ALL of them.** No GPU handle was created.

**Conclusion:** `isEnabled=1` ([[cp2077-rttcam-isenabled-is-the-flag]]) is NECESSARY but NOT SUFFICIENT. These cams live on `entEntityPreview` entities that are not render-active while their UI is closed, so the render scene never visits them — enabling the flag alone does nothing. The working preview cam's handle comes from its ENTITY being render-active (the inventory's RTT-view setup driver), not from isEnabled. Confirms [[cp2077-rttcam-census-no-player-cam]] reasoning and the no-drivable-API wall ([[cp2077-rtt-no-drivable-api]]).

**Remaining definitive RTT test (path 1):** put an RTT cam on an ALWAYS render-active entity (player) and enable it. If a handle appears -> native RTT works. If not -> the RTT render is gated on the preview-view driver and the path is dead -> pivot to option-C node-executor re-exec ([[cp2077-framegraph-node-executor-hookable]]). Caveat: even a player-attached enabled RTT cam may need an explicit RTT-view registration (the driver), not just being on a rendered entity.
