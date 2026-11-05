# Captures and the controller harness (OpenXR Simulator)

Pictures and clips of the game, and scripted controller input, go through `tools/sim_harness/simxr.py`. Do not
use the simulator window's `screenshot.bmp` / `screenshot_request.json` for results any more: that is the mirror
window at its own size and zoom (a 1920x310 strip when the window is a strip), the eyes squeezed into it.

## What it captures

The simulator composes what the runtime receives -- both eye projections and every HUD quad layer -- into one
render target. While a capture is active that target is sized to the eye's own aspect (the projection layer's
sub-image rect) and the eyes fill it edge to edge, so a still or a clip is exactly what each eye of a headset
gets, HUD included, at the eye's resolution:

| request | output | measured (Quest 3 profile, 2648x2864 per eye) |
|---|---|---|
| `shot(path, eyes='both')` | `path_L.jpg`, `path_R.jpg`, `path_SBS.jpg` (JPEG 4:4:4 q95, or `fmt='png'`) | 5296x2864 SBS, 0.3 s |
| `shot(path, eyes='left', eye_size=1400)` | `path_L.jpg`, eye scaled to 1400 px high | 1294x1400 |
| `record_start(path.mp4, eyes='both', eye_size=1920, fps=30)` | HEVC MP4 (hardware Media Foundation encoder) | 3520x1920@30, 8 s = 15.9 MB, 0 frames dropped |
| `record_start(..., eyes='left', eye_size=1400)` | one eye | 1280x1392@30, 3.7 Mbps |

- Video sizes are multiples of 16 (an odd stride sheared the picture); stills are even.
- HEVC is written into plain MP4 (Media Foundation refuses HEVC in fragmented MP4: `MF_E_INVALIDMEDIATYPE`
  at `BeginWriting`). H.264 is the fallback (fragmented MP4 first, so a killed game leaves a playable file), and
  above 4096 px wide there is no H.264. `codec='h264'` forces it; `mbps=0` lets the encoder pick from the size.
- `max_seconds` stops a recording by itself (a test that dies mid-clip still gets a closed file).
- While recording, the simulator window repaints one frame in eight: the readbacks go to the encoder.
- `tools/sim_harness/vframe.exe CLIP OUT.jpg [SECONDS]` decodes one frame (codec, size, fps and duration on
  stdout) -- the way to look at a clip from a script.

The runtime side is `build/hud-preview-runtime/src/capture.h` (simulator source), driven by
`%LOCALAPPDATA%\OpenXR-Simulator\capture_request.json` and answered in `capture_status.json`. `simxr.py` writes
the requests and waits for the answers.

## The controller harness

`Sim()` attaches to the newest `Cyberpunk2077.exe`, finds `openxr_simulator.dll` in it and reads the harness
exports (`SimXR_HarnessVersion` 2, `SimXR_Controllers[2]`, `SimXR_ControllerLayout`). Per hand it writes:

- `hand(which, pos, quat | ypr, mode)`: full pose. `mode='body'` (default): position is the offset from the head
  turned by the head's yaw only, rotation in that yaw frame. `mode='world'`: both in tracking space.
  `ypr` = yaw about +Y (left positive), pitch about +X (up positive), roll about the pointing axis -Z (top to the
  right positive). Verified in the game: rolls 0 / 90 / 180 / 270 turn the hand about its pointing axis through
  four distinct orientations, and yaw 45 turns it about the vertical.
- `glide(...)`: the same, interpolated (slerp) over `seconds`.
- `trigger`, `grip` (0..1), `button(which, 'a'|'b'|'x'|'y'|'menu'|'stick'|'trigger'|'grip', on)`, `stick(x, y)`,
  `tap(which, name)`, `restore()` (hands back to the simulator's defaults).
- `head(yaw, pitch, roll, pos)`: degrees and metres.

The game must not have the foreground while a test drives the pad: focused, the simulator turns keyboard and
mouse into controller input every frame. `Sim()` hands the focus away.

## Rules for tests

- Shoot each eye at its own aspect (`shot`, `record_start`), never a squeezed composite.
- Close the game after a test; Blender and RenderDoc only while it is closed.
- Restore the simulator window size in `%LOCALAPPDATA%\OpenXR-Simulator\settings.json` if a test changed it.
