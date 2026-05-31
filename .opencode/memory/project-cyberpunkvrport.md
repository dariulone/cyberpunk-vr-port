---
name: project-cyberpunkvrport
description: What the CyberpunkVRPort project is — VR mod (dxgi.dll proxy) for Cyberpunk 2077
metadata: 
  node_type: memory
  type: project
  originSessionId: 0353ad85-8636-45bd-8b7d-635741e848f9
---

CyberpunkVRPort is an open-source VR mod for Cyberpunk 2077, shipped as a `dxgi.dll`
proxy dropped into `bin/x64/`. It injects OpenXR (D3D12) into REDengine via pattern-scan
(AOB) hooks — **not** RED4ext. F10 opens an ImGui settings overlay; settings persist to
`vrport.ini` in the game dir.

Key source (all under `src/`):
- `dxgi_proxy.cpp` (~3.5k lines): proxy entry, AOB hooks (camera, FOV, LOD, DLSS res),
  live-controls/settings system, head-pose → in-game camera mapping.
- `openxr_manager.cpp` (~1.5k lines): OpenXR session, swapchains, the AER frame thread
  (`FrameThreadMain`), present capture (`OnPresent`/`CapturePresentedFrame`), `GetHeadPose`.
- `imgui_overlay.cpp`, `launcher_dialog.cpp`, `dxgi_factory_wrapper.cpp`, `camera_hook.h`.

Current known-good baseline (2026-05-31):
- **Mono mode is smooth and usable.** The big fix was binding the submitted pose to the
  exact camera/render sequence id instead of guessing pipeline lag. The root issue was
  async pose/frame mismatch; it was fixed by storing render poses keyed by the game's
  camera `seq` and fetching the exact pose for the frame that was actually rendered.
- **AER now has working stereo depth in-world** after the same kind of sequence-based
  correction was applied to the eye index: the eye slot used at submit now comes from the
  actual rendered camera `seq`, not the current live eye counter. This removed the
  left/right alternation-without-depth bug.
- **AER parallax is driven by `OnLocateCameraCallback`, not `OnPatchCameraCallback`.**
  `OnPatchCameraCallback` does not drive the final render camera for stereo separation;
  the working IPD shift is injected in `OnLocateCameraCallback`.
- **Current orthoscopic AER sign:** eye0 -> `-right`, eye1 -> `+right` in the
  LocateCamera IPD block. Earlier flipped sign became pseudoscopic after the eye-tag fix.
- **AER menus use mono fallback intentionally.** When the in-game menu is active,
  `FrameThreadMain` uses the already-working mono snapshot/quad path instead of waiting
  for an AER stereo pair. This fixes the black-screen-in-menu bug with AER enabled.
- **`xr_aer_debug_eye` supports 0..3 again.** It was previously clamped to 0/1 in two
  places, which invalidated several diagnostics until fixed.

Stereo is **AER** (Alternate Eye Rendering): one eye per game frame. Present capture
alternates eyes; the frame thread submits L/R pairs to OpenXR.

Goal: bring AER up to "AER 2.0" quality like Luke Ross's RealVR.dll. See [[aer-roadmap]].
Build: see [[build-cyberpunkvrport]]. Adding a setting: see [[settings-plumbing]].
Owner/branch: work happens on `dev`. Testing HMD: PICO 4 via VDXR (user has the headset; I cannot test VR).
