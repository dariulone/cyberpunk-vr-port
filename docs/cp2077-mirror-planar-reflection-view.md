---
name: cp2077-mirror-planar-reflection-view
description: apartment mirror = worldMirrorNode planar-reflection = real persistent gameplay 2nd full-scene view, BUT reduced pass-set (opaque/no-TXAA) + mirrors-area gated => proof not vehicle
metadata:
  type: project
---

Live recon (2026-06-09, user in V's apartment, can look around in mirror+monitor in VR = real-time 3D renders, not video): the heap scan for entRenderToTextureCameraComponent (needle exeBase+0x307BFD0) finds ONLY idle preview cams + false positives in gameplay -- the mirror/monitor are NOT active RTT-camera components. They are a DIFFERENT mechanism.

The apartment mirror = **worldMirrorNode / worldMirrorNodeInstance** (strings @0x142DD99F0/0x142DD9AF0), rendered as a **planar reflection**: render stages `renderstage_planar_reflection` (@0x142B10C48) and `renderstage_depthprepass_mirror_opaque_notxaa` (@0x142EA3AB8). FrameGraph builder = **sub_14078A6B8** (0xC22, references the planar_reflection string x2; no static callers = virtual FrameGraph dispatch; many callees in 0x1Fxxxx = FG pass-add helpers). Proxy data type = RenderProxyCustomData_Mirror. Gated by "mirrors area" (m_isInMirrorsArea / questEntityManagerToggleMirrorsArea -- quest-toggled, not open-world).

DECISIVE judgment for true-stereo: the mirror PROVES the engine natively schedules a 2nd full-scene view into the view fan-out IN GAMEPLAY, persistently, with its OWN per-view pass set (=> fan-out supports per-view pass configuration; a full-quality 2nd view is architecturally allowed). BUT the mirror is NOT a usable right-eye vehicle: its pass set is REDUCED (opaque-only, no TXAA, no transparents) and it is mirrors-area gated. Using it as an eye would give unequal-quality eyes = the very "fake" the user rejected.

Net landscape: engine 2nd-view producers split into (a) full-quality but handle/render-attach walled = RTT camera (35 sessions); (b) easily-triggered but reduced/gated = mirror. No free full-quality gameplay vehicle. Path to equal-quality stereo still requires INSERTING our own full-pass-set view into the fan-out (engine view-insert, which manual doubling tears) OR render-attaching a full-quality RTT camera (handle wall). User requirement (2026-06-09): genuine simultaneous synchronous EQUAL-quality eyes; rejected AER (ghosting), reprojection, and any reduced-quality 2nd view. Links: [[cp2077-view-fanout-submit-chain]] [[cp2077-per-view-independent-fgbuild]] [[cp2077-loaddyntex-handle-producer]] [[cp2077-producer-double-mutates-mgr]].
