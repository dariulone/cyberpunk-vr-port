
# CyberpunkVRPort

OpenXR `dxgi.dll` VR proxy for **Cyberpunk 2077**.

Repository: <https://github.com/dariulone/cyberpunk-vr-port>

`F10` opens the in-game VR menu.

## Current State

### Mono
- Working and smooth.
- Head-locked jitter/rubber-banding was fixed by binding the submitted pose to the
  exact render-camera sequence used by the engine.
- This is the current stable baseline.

### AER
- Working stereo depth in-world.
- Fixed issues in the current branch:
  - first-pair parity freeze
  - Mono -> AER frame-thread stall
  - rendered-eye/slot mismatch that caused visible alternation instead of stereo depth
  - pseudoscopic depth (wrong parallax sign)
  - black menu in AER (menu now falls back to the mono quad path only while menu is open)
- `AER V2` / optical-flow interpolation is still separate from the normal AER baseline.

## Features

- Direct OpenXR integration inside the REDengine render path.
- Mono and AER rendering modes.
- Head tracking with in-engine camera injection.
- Automatic runtime FOV-based projection handling.
- LOD / culling corrections for VR camera movement.
- VR menu quad mode.
- Pre-launch render-resolution selector.
- Runtime and hardware diagnostics in the log:
  - OpenXR runtime name / kind / version
  - OpenXR system name / vendor / tracking capabilities
  - GPU name / vendor / device id / VRAM / driver version
- Built-in logging for tester reports.

## Installation

1. Download `dxgi.dll` from Releases.
2. Place it into `Cyberpunk 2077\bin\x64\` next to `Cyberpunk2077.exe`.
3. Start your OpenXR runtime first.
4. Launch the game.

## Startup / Runtime Notes

This mod uses **OpenXR**.
The mod now logs which runtime is active

### Virtual Desktop / PICO
- Start Virtual Desktop / VDXR first.
- Then launch the game normally.
- If the game launches as a flat desktop window inside the headset, check the log to see
  which OpenXR runtime was selected.
  
## Resolution Selection

At startup, the launcher dialog lets you pick a render resolution preset.

## Controls

- `F7` recenter
- `F10` open in-game VR settings

This split is intentional so test logs and user configs stay cleaner.

## Logs

Main log file:

- `Cyberpunk 2077\bin\x64\CyberpunkVRProxy.log`

Useful things now logged automatically:
- OpenXR runtime and version
- OpenXR system name and tracking capability
- GPU model and driver version
- swapchain / session initialization
- Mono / AER frame-pipeline events

This is the preferred file for community bug reports.

## Media

[![Cyberpunk VR Short 1](https://img.youtube.com/vi/Q_nt0dceXNU/0.jpg)](https://www.youtube.com/shorts/Q_nt0dceXNU)
[![Cyberpunk VR Short 2](https://img.youtube.com/vi/CXeYW1_FTWE/0.jpg)](https://www.youtube.com/shorts/CXeYW1_FTWE)

![IMG_6564](images/IMG_6564.jpg)
![IMG_6566](images/IMG_6566.jpg)
![IMG_6570](images/IMG_6570.jpg)
![IMG_6573](images/IMG_6573.JPG)

*Photos were taken through PICO 4 lenses using an iPhone 13 Pro Max.*

## Test Hardware Used During Development

- Headset: PICO 4 (via VDXR)
- CPU: AMD Ryzen 7 5800X
- GPU: NVIDIA RTX 5070 Ti
- RAM: DDR4 32GB
- OS: Windows 11 Pro 25H2 26200.8457
