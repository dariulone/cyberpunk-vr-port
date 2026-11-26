# Verify the monitor eye in a game frame

Use a stationary gameplay view with a nearby object against a distant edge. Capture one frame with **Left eye** selected, then another run with **Right eye**. Each run supplies its own references. A shared inventory, loading screen or genuinely mono frame cannot establish handedness.

## Identify the window and its physical client

Run this while your game is showing the scene.

```powershell
$game = Get-Process -Name Cyberpunk2077
python tools/roomscale_tests/capture_preview.py --window game --pid $game.Id --inspect
```

The helper selects the visible **W2ViewportClass** for that PID. Check that `minimized` is false and match its handle to the plugin log entry `Present: bound overlay to game window` or the `hwnd` in `Present result`. That handle is the game swapchain's `OutputWindow`. The half-open `clientScreenRect` gives physical screen pixels, including negative origins on secondary monitors. The simulator preview, launcher and optional VRCAM window have different handles.

## Read the image presented by the game

Use a D3D12 frame capture of the identified game swapchain with the port's normal renderer active. Finish acquisition and stop the game before opening the saved capture for replay. At the game swapchain's **Present** event, export the presented color resource at mip 0, slice 0, at native size. This is the game backbuffer after the monitor copy, desktop overlay and any desktop crop, before Windows scales or composites the window.

For this byte comparison, close the desktop overlay and use a frame in which the desktop crop pass did no blit. The normal auto-fitted client has the backbuffer aspect. A custom client aspect can trigger a final resampling draw. Check the event list for writes to the presented buffer between the mirror copy and Present. Export every texture as RGB(A) PNG with the same UNORM channel interpretation, transfer function and range. The verifier ignores alpha.

- **Native side-by-side, simplest test.** Use `xr_sharpmix=0` for this run so the eye copies keep the original RGB values. Export the **same backbuffer resource** at the event immediately before the mirror clears its margins or overwrites its pixels, after the game has finished both eyes. Save that as `packed.png` and the Present export as `presented.png`. The left reference starts at column 0. The right starts at `W - floor(W / 2)`; each is `floor(W / 2)` columns wide.
- **With sharpening or separate eye textures.** Export both completed eye textures after composition and sharpening, before the pool slots are reused. Label them independently of the monitor setting. For native stereo, follow the copies from their left and right source boxes. For VRCAM, MAIN's side comes from `CyberpunkVR_MainIsRightEye` and the other side from the composed second-eye slot. For AFW, use that frame's drawn-eye identity. MAIN can be either physical eye. A reference taken from the monitor copy alone cannot establish which physical eye was chosen.

```powershell
python tools/launcher_tests/verify_monitor_frame.py left presented.png --packed packed.png
python tools/launcher_tests/verify_monitor_frame.py right presented.png --eyes left.png right.png
```

A pass requires exact RGB equality throughout the eye rectangle, distinct eye references, scene detail in the selected reference and black margins. Exit codes are 0 for pass, 1 for a pixel mismatch and 2 for inconclusive input. As a negative control, request the opposite eye using the same files. It should return 1 and report `matches_opposite_eye` as true.

## Geometry and desktop screenshot checks

For a backbuffer of size `W × H` and a finished eye of size `E × H`, the eye occupies `[floor((W-E)/2), floor((W-E)/2)+E) × [0,H)`. In the separate-eye path, `E=W` fills the buffer. For a 4000 × 2000 native stereo backbuffer the eye occupies columns 1000–2999. On a 1600 × 800 client with matching aspect, those are client columns 400–1199. Add the physical client origin for screen coordinates.

If the desktop crop is active, use the visible source rectangle from `ComputeDesktopCoverTransform`. Subtract its origin from the eye edges, scale by client size divided by visible source size, then clip to the physical client. A capture taken **at Present** already includes that crop. Use native backbuffer coordinates for the exact comparison above.

The existing `tools/roomscale_tests/capture_preview.py --pid <PID> --window game --out <fresh.png>` calls Win32 `PrintWindow` on the outer window. It can provide a visual cross-check. It can also return black, and its pixels cannot verify the pre-compositor backbuffer. The standalone `monitor_output_pixels` CTest exercises the same D3D12 mirror copy and reads the resulting pixels on WARP.

### Live results for both launcher choices

Both launcher choices showed the selected eye on the monitor. Each run captured a 2560 × 1384 game window with an active eye rectangle spanning columns 640–1919 and the full height. The test compared image gradients in the interior of that rectangle with both OpenXR Simulator eye images from the same run.

| Launcher choice | Game PID | Correlation with left eye | Correlation with right eye | Local result file |
| --- | ---: | ---: | ---: | --- |
| Left eye | 14956 | **0.9542** | 0.0622 | `build/physical-melee/display-left/result.json` |
| Right eye | 27532 | 0.0628 | **0.9346** | `build/physical-melee/display-right/result.json` |
