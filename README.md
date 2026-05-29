# CyberpunkVRPort
This is a open-source VR implementation proxy (dxgi.dll) for Cyberpunk 2077.
Press F10 ingame to open VR settings menu.

## Features & Fixes
- Added OpenXR integration directly into the Cyberpunk engine.
- Implemented **Fix Head** movement - decouples head from the player body for comfortable 3DoF/6DoF headset tracking without the body models aggressively pulling the camera.
- Overridden engine FOV properly via runtime hooks to calculate correct FOV for arbitrary VR aspect ratios.
- Decoupled `lod_fov_override` to fix shadow and object culling when turning the head in VR. The LOD system is tricked into thinking the FOV is narrow to prevent aggressive high-detail mesh culling under the feet.
- Fixed UI positioning and interaction logic.
- Hooked `CP2077SettingsRes` and `CP2077DLSSRes` to force resolutions properly suited for VR headsets, avoiding stretched or compressed views.
- Synchronized rendering pipeline (Engine settings, DLSS, DXGI Display Modes, and Swapchain) to support custom VR resolutions without breaking tracking, aspect ratio, or creating fish-eye effects.
- Added a pre-launch dynamic resolution selector dialog to cleanly set rendering sizes before the engine initializes.

## Installation
1. Download `dxgi.dll` from the Releases page.
2. Place `dxgi.dll` into the game directory where the `Cyberpunk2077.exe` is located (usually `Cyberpunk 2077/bin/x64/`).
3. Start the game.

## Media
Videos:
[![Cyberpunk VR Short 1](https://img.youtube.com/vi/Q_nt0dceXNU/0.jpg)](https://www.youtube.com/shorts/Q_nt0dceXNU)
[![Cyberpunk VR Short 2](https://img.youtube.com/vi/CXeYW1_FTWE/0.jpg)](https://www.youtube.com/shorts/CXeYW1_FTWE)

Images:
![IMG_6564](images/IMG_6564.HEIC)
![IMG_6566](images/IMG_6566.HEIC)
![IMG_6570](images/IMG_6570.HEIC)
![IMG_6573](images/IMG_6573.JPG)

*Note: All pictures were taken through PICO 4 lenses using an iPhone 13 Pro Max.*

## Testing Configuration
- **Headset:** PICO 4 (via VDXR)
- **CPU:** AMD Ryzen 7 5800X
- **GPU:** NVIDIA RTX 5070 Ti
- **RAM:** DDR4 32GB
- **OS:** Windows 11 Pro 25H2 26200.8457
