---
name: cp2077-rttcam-census-no-player-cam
description: Live census - 20 RTT cams in process, ALL on preview/photomode entEntityPreview entities, NONE on the player; mod's redscript adds no RTT cam
metadata:
  type: project
---

Live-RPM heap scan (tools/scan_rttcam.ps1, vtable exe+0x307BFD0) with inventory CLOSED:
- 20 RTT-cam instances found (1 was a garbage vtable-qword false-positive).
- ALL 19 real ones: `comp+0x88 = 0x3` (isEnabled=0), `comp+0x1E8 = 0` (no handle) — nothing attached while no preview UI is open.
- One has resref `0xD55FB557EAEBA05B` = the inventory preview cam (same resref as the working cam in [[cp2077-rttcam-attached-vs-not-live]]), currently detached.
- `ForceEnableRTTCamera` reported **0 RTT cams on the player** — the player has none.

Project-side check: the mod's redscript (`mods/redscript/...`) adds ONLY HUD; it does NOT add any entRenderToTextureCameraComponent. So nothing is currently set up for native RTT stereo. Earlier plugin RTT functions (TryCreateRTTTarget, DriveRTTCameraLoad, etc.) assumed a player RTT cam that isn't there (likely a CET/Lua-added test cam in past sessions, not committed).

**Implication for native RTT True Stereo path:** all preview cams live on `entEntityPreview` entities that are only render-active when their UI is open, so enabling one while its UI is closed may not produce a handle (entity-active is a confound). The real path needs an RTT cam on an ALWAYS-render-active entity (the player), which means: (1) add the component to the player, (2) get it through render-attach (the long-standing wall), (3) isEnabled=1 + target texture. Step (2) remains the open blocker. If even a properly-added+enabled player cam won't attach, the RTT path is dead -> pivot to option-C node-executor re-exec ([[cp2077-framegraph-node-executor-hookable]]).

Plugin now has `ForceEnableRTTCamera(w,h)` (player cams) and `ForceEnableRTTCameraAt(lo,hi)` (specific instance, vtable-validated) for enable experiments.
