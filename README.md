# CyberpunkVRPort

OpenXR `dxgi.dll` VR proxy for **Cyberpunk 2077**, now with **6-DoF motion-controlled VR hands**
(full-arm VRIK) and an in-headset settings overlay.

Repository: <https://github.com/dariulone/cyberpunk-vr-port>

`F10` opens the in-game VR menu.

## Documentation

See [`docs/`](docs/) for engineering notes. In particular
[`docs/stereo-rnd.md`](docs/stereo-rnd.md) documents the extended research into
*genuine simultaneous stereo* (D3D12 command replay and geo-11-style shader
injection), why it did not ship on REDengine's deferred renderer, and the
realistic paths forward. The full experimental working tree is preserved as a
reversible patch (`docs/stereo-rnd-experiment.patch`).

## Current State

### Stereo / Camera
- **Mono** — working and smooth. Head-locked jitter/rubber-banding was fixed by binding the
  submitted pose to the exact render-camera sequence used by the engine. This is the stable baseline.
- **AER** — working stereo depth in-world (first-pair parity freeze, Mono→AER stall, eye/slot
  mismatch, pseudoscopic sign, and the black AER menu were all fixed). `AER V2` / optical-flow
  interpolation is still a separate experimental track.

### VR Hands (new)
- **Full VRIK-style VR hands** driven directly by the motion controllers, with a complete
  **shoulder → elbow → hand** IK chain. The forearm no longer stretches; the wrist orientation,
  elbow bend and elbow swivel follow the controller naturally.
- Tracking runs as a **RED4ext plugin** (`CyberpunkVR_Hands.dll`) that writes the resolved bone
  rotations every frame; controller poses are bridged from the proxy through shared memory.
- **Weapons work** while tracked.

## Features

- Direct OpenXR integration inside the REDengine render path (Mono + AER).
- Head tracking with in-engine camera injection and runtime FOV-based projection handling.
- **Motion-controlled VR hands** with full-arm VRIK (VRArmIK-style elbow swivel heuristic).
- **In-headset F10 overlay** with separated tabs:
  - **VRIK** — start/stop hand tracking, live IK calibration (per-hand reach scale, height,
    elbow swing, elbow pole, wrist rotation offset), Log VR Diag.
  - **HUD** — live VR HUD layout (per-element X / Y / Size on a single compact row).
  - **Debug Gizmos** — hand overlay / proxy / debug axes / locator scale.
  - **Tracking / Camera** — movement-control mode and recenter.
- **Head-oriented locomotion** — optional *Movement Control: HMD* so on-foot movement follows where
  you look (driving is untouched).
- SteamVR (OpenVR) runtime support, selectable alongside OpenXR.
- Pre-launch render-resolution selector.
- Runtime/hardware diagnostics in the log (OpenXR runtime + system, GPU model + driver, swapchain
  init, frame-pipeline events).
- **Verbose-log toggle** — the log is quiet by default for clean tester reports; deep per-frame
  diagnostics are behind one checkbox.

## Requirements

- **Cyberpunk 2077** (PC).
- An OpenXR runtime (e.g. Virtual Desktop / VDXR, SteamVR) — start it **before** the game.
- For VR hands and the VR HUD:
  - **RED4ext** — loads `CyberpunkVR_Hands.dll`.
  - **Cyber Engine Tweaks (CET)** — runs the `CyberpunkVRPort_VRIK` and `CyberpunkVRPort_HUD` mods.

## Installation

1. Install **RED4ext** and **Cyber Engine Tweaks** if you don't have them.
2. Download the latest release archive and extract it into your **Cyberpunk 2077** game folder so the
   files land in:
   - `bin\x64\dxgi.dll`
   - `bin\x64\openvr_api.dll` *(only needed for the SteamVR path)*
   - `bin\x64\plugins\cyber_engine_tweaks\mods\CyberpunkVRPort_VRIK\`
   - `bin\x64\plugins\cyber_engine_tweaks\mods\CyberpunkVRPort_HUD\`
   - `red4ext\plugins\CyberpunkVR_Hands\CyberpunkVR_Hands.dll`
3. Start your OpenXR runtime first, then launch the game.

> Proxy-only install (camera/stereo, no hands): just drop `bin\x64\dxgi.dll` next to
> `Cyberpunk2077.exe`. The VR hands/HUD additionally need RED4ext + CET as above.

## Startup / Runtime Notes

This mod uses **OpenXR**; the log reports which runtime is active.

To force the OpenXR path through SteamVR, set `xr_runtime=1` in `vrport.ini` and restart. The loader
tries `openvr_api.dll` first, then the local Steam install, and sets `XR_RUNTIME_JSON` to SteamVR's
OpenXR manifest.

### Virtual Desktop / PICO
- Start Virtual Desktop / VDXR first, then launch the game normally.
- If the game opens as a flat desktop window inside the headset, check the log for the selected
  OpenXR runtime.

## Controls

- `F7` — recenter
- `F10` — open the in-game VR settings overlay

## Logs

Main log file:

- `Cyberpunk 2077\bin\x64\cyberpunkvrport.log`

It is quiet by default (startup + key events only). For deep diagnostics, enable
**F10 → DLSS/Debug → Verbose log**. Runtime `.txt` diagnostic dumps are written next to the game exe.

This is the preferred file for community bug reports.

## Media

[![Cyberpunk VR Short 1](https://img.youtube.com/vi/Q_nt0dceXNU/0.jpg)](https://www.youtube.com/shorts/Q_nt0dceXNU)
[![Cyberpunk VR Short 2](https://img.youtube.com/vi/CXeYW1_FTWE/0.jpg)](https://www.youtube.com/shorts/CXeYW1_FTWE)

![IMG_6564](images/IMG_6564.jpg)
![IMG_6566](images/IMG_6566.jpg)
![IMG_6570](images/IMG_6570.jpg)
![IMG_6573](images/IMG_6573.JPG)

*Photos were taken through PICO 4 lenses using an iPhone 13 Pro Max.*

![Boe6Eod7Nty Valve Index](images/Boe6Eod7Nty_valve_index.jpg)
![Boe6Eod7Nty Valve Index 2](images/Boe6Eod7Nty_valve_index_2.jpg)

*Valve Index (canted-display OpenXR) shots courtesy of [Boe6Eod7Nty](https://github.com/Boe6Eod7Nty).*

## Test Hardware Used During Development

- Headset: PICO 4 (via VDXR)
- CPU: AMD Ryzen 7 5800X
- GPU: NVIDIA RTX 5070 Ti
- RAM: DDR4 32GB
- OS: Windows 11 Pro 25H2 26200.8457
