---
name: cp2077-rtt-no-drivable-api
description: decider result — RTT target+driver wiring is NOT exposed in RTTI; native-C front needs C++ driver reverse (R2)
metadata:
  type: project
---

CP2077 option-C (true simultaneous stereo) FRONT decider, 2026-06-07: RTTI dump (reflection_rtti_dump.txt) shows **0 wiring-function hits** — no free/global function takes or returns DynamicTexture/RenderToTexture. The 3 method hits are consumers only (gamedataChannelData_Record::DynamicTexturePath read-only; inkImageWidget::SetDynamicTextureUV displays a ready texture).

Driver classes (gamePhotoModeSystem, gameuiHolocallCameraComponent + questHolocallStateListener, gameuiInventoryPuppetPreviewGameController / entEntityPreview / RenderTexturePreviewData) all have the DynamicTexture-target + render-enable wiring **baked into C++ game systems**, not callable from red4ext/CET. Photomode takes over the main view (not a 2nd simultaneous view); preview renders an isolated entity on a special stage (not world scene from arbitrary camera); holocall is quest-driven.

Conclusion: native RTT mechanism is real but cannot be **driven by our own code** via reflection. Standing-up a 2nd world view requires R2 = author a DynamicTexture resource via ArchiveXL **and** reverse the holocall/preview C++ driver to reproduce bind+enable. Multi-week, chained unknowns (grab -> pose -> compose -> TAA -> perf), no guarantee preview stack can even render full world from arbitrary camera. Corrects earlier false-positive [[cp2077-rtt-camera-native-stereo]] ('FRONT solved'). See also [[cp2077-952xx-is-generic-task-scheduler]].
