# Fixed and Eye-Tracked Foveated DLSS

General > Foveated Rendering uses the port's own D3D12/NGX implementation.
The center inherits the game's DLSS/DLAA quality and render-preset hints. Only
peripheral quality is selected in the overlay. The game's render resolution,
graphics settings and DLSS configuration are not changed.

This reduces DLSS reconstruction work, not geometry, raster shading or ray
tracing work. It is not VRS. NVIDIA RTX and active game DLSS Super Resolution
or DLAA are required. Both **Fixed** and optional **Eye-tracked (OpenXR)**
(`XR_EXT_eye_gaze_interaction`) region tracking are supported with no external
proxy DLL or third-party OpenXR layer required; when eye tracking is
unavailable, unbound, or loses tracking, the center automatically falls back to
the configured center offsets. Ray Reconstruction uses a separate NGX feature
and is not implemented.

## Controls

| Peripheral quality | Peripheral DLAA resolution relative to game input |
| --- | --- |
| Off (default) | Original full-frame game DLSS, no private passes |
| Quality | 100% per axis |
| Balanced | 75% per axis |
| Performance | 50% per axis |

These names describe peripheral profiles, not a change to the game's DLSS
setting. For example, with game DLSS Balanced, the center remains Balanced in
all three profiles. Peripheral output is reconstructed at the smaller
resolution and spatially scaled to the eye image.

`Region tracking` selects `Fixed` (default) or `Eye-tracked (OpenXR)`. In
`Eye-tracked (OpenXR)` mode, the port queries `XR_EXT_eye_gaze_interaction`
directly and steers each eye's central crop dynamically, holding through brief
blinks before decaying smoothly back to `Center offset X/Y` on extended
tracking loss.

`Center width` and `Center height` independently set 20..100% of each eye image,
with defaults of 70% width and 65% height. `Center offset X/Y` shifts the region in percentages
of the eye image (and acts as the fallback position in eye-tracked mode); zero
is centered, positive X moves right, and positive Y moves down toward the
hands. Travel is limited by the selected region size so the entire region
remains in bounds. X defaults to 0%, Y to -10%.

Peripheral quality (`mode`) and dimension (`Center width` / `Center height`)
changes apply once, one second after editing stops. Holding a slider still or
using numeric entry keeps the previous region active until release, followed by
the full one-second delay. Rapid INI changes to those allocation settings are
also debounced. Closing the panel ends the hold. `Off`, `Show region boundary`,
`Region tracking`, and same-size `Center offset X/Y` changes apply immediately
without rebuilding private DLSS features.

A 4% feather blends the two results. `Show region
boundary` enables a diagnostic border. At 100% on both axes, the private center
covers the full frame; select Off to bypass all private processing overhead.

Settings in `vrport.ini`:

- `xr_foveated_dlss=0`: Off/Quality/Balanced/Performance = 0/1/2/3.
- `xr_foveated_width=70.00`: central width percentage.
- `xr_foveated_height=65.00`: central height percentage.
- `xr_foveated_boundary=0`: diagnostic border.
- `xr_foveated_offset_x=0.00`: horizontal center displacement in percent.
- `xr_foveated_offset_y=-10.00`: vertical center displacement in percent, positive down.
- `xr_foveated_eye_tracked=0`: region tracking (`Fixed` = 0, `Eye-tracked (OpenXR)` = 1).

The former `xr_foveated_edges` key migrates to `100 - 2*edges` on each axis.
Explicit width/height keys take precedence regardless of INI line order.

## Rendering

Each eye has independent center and peripheral NGX features and temporal
histories. Region dimension or peripheral resolution changes rebuild these
features. Same-size offset changes and dynamic eye-gaze shifts reposition the
crop without rebuilding NGX features, applying per-frame central motion-vector
compensation with a large-saccade history reset guard (>60% of center input
width or height). Ordinary head movement does not reset their histories.
Disabling foveation resets the game's original DLSS history once before
resuming normal full-frame processing.

The center is cropped on the original input pixel grid. Color, depth and motion
are packed into zero-origin private textures. Depth and motion use matching
samples; motion is converted to local pixel units. Center jitter is unchanged,
while peripheral jitter follows the downsample ratio. Composition accounts for
the fractional output position of the input-aligned crop. Game render-preset
hints are captured at NGX feature creation and reused for the private center.

Temporarily overridden NGX parameters are restored after each private call.
Resources and immutable descriptors are retained with the command-list batch
until GPU completion. NGX feature release is deferred outside that registry's
lock. Unsupported inputs and private evaluation failures fall back to native
DLSS. Processing is bypassed in menus and outside the identified eye passes.

The optional Cheeky runtime used for the initial comparison is no longer loaded
or packaged. Its design was a research reference:
[CheekyFoveatedDLSS](https://github.com/ClarkCheekyKent/CheekyFoveatedDLSS), revision
`d6c18b23d31f7339735114c621bc3f9066e1a32d`. This backend uses the public
[NGX parameter API](https://github.com/NVIDIA/DLSS/blob/main/include/nvsdk_ngx_params.h).

## Validation

Build the port normally. No separate backend build is required.

```powershell
cmake -S tools/foveated_tests -B build/foveated-tests -A x64
cmake --build build/foveated-tests --config Release
ctest --test-dir build/foveated-tests -C Release --output-on-failure
```

The settings test covers independent dimensions, bounds, invalid values,
legacy migration and INI round-trip. The NGX test checks parameter restoration
and game-preset inheritance through substituted handles. The WARP GPU test checks exact source-grid
color samples, depth, local motion-vector scaling and composite pixels with the
D3D12 debug layer enabled; warnings and errors fail the test.

Launch into a save with `tools/quickboot_tests/launch_to_save.py`, then:

```powershell
py -3.13 -B tools/foveated_tests/live_probe.py --pid <PID> --out <fresh-directory> --width 70 --height 40 --motion 12
```

The bounded probe requires fresh world-ready and simulator status, monitors
crash reports/reporters, reads exported diagnostics using read-only process
access, tests all profiles and Off, captures both eyes, supplies smooth head
movement and verifies that both histories advance without resets. It restores
the original INI and pose. `--smoke --framegen` tests a region with frame
generation enabled. GPU timings measure this DLSS path per evaluation, not
total frame time or an equivalent FPS improvement.
`--offset-y 15 --debounce` additionally checks a lowered region and rapid
settings changes without intermediate DLSS rebuilds. `--eye-gaze` drives
synthetic OpenXR eye gaze (smooth pursuit, saccade step, blink hold, and
extended dropout) via exported test controls while keeping default runs
read-only, and always restores the override and INI afterward. The settings
test uses a deterministic clock to cover stationary holds, the release delay,
repeated INI polls, immediate Off, and image-edge clamping.

On 2026-09-30, the native preset-inheritance build (SHA-256
`4c373e3f0812bf7a70191fb99d3054c9b7d92df4a377254fd32cb6cb90e7e11e`)
passed all three tests and two simulator runs in PID 21940. Both eyes inherited
all six game render-preset hints; game Balanced input was 1485x1485 and output
2560x2560. A 70% x 40% region used a 1040x594 input crop and 1793x1024 private
center output. Quality, Balanced, Performance and Off completed with zero
evaluation failures. Smooth head movement did not reset either history.
The framegen run completed 1,423 evaluations per eye with one initial reset,
and its stereo screenshot showed 45 real + 45 generated FPS.

Medians of the final 20 diagnostic samples of each non-framegen stage:

| Profile and center region | DLSS-path GPU time per measured evaluation |
| --- | --- |
| Quality, 70% x 40% | 0.91 ms |
| Balanced, 70% x 40% | 0.81 ms |
| Performance, 40% x 40% | 0.61 ms |
| Off, native full frame | 1.26 ms |

The Performance stage also uses a smaller center, so it is not an isolated
comparison of peripheral profiles. These are bounded tests of execution and
resource mapping, not proof that head-motion shimmer is visually eliminated.
Hardware headset visual quality still requires user confirmation. Captures and
reports are under `build/foveated-native/21940-modes` and `21940-framegen`.

The offset/debounce build
`c15fbc01a9217d09160da017062e1e5d37c434a5fc29284006545758908ca7a6`
passed 3 foveation and 16 overlay tests, including a three-second stationary
hold of the actual grouped slider widget. PID 19920 then passed the live
offset/debounce probe: eight rapid updates left the previous crop unchanged;
only the final region was applied after settling. X=-10%, Y=+20% moved the
60% x 50% region to output origin 255,1151 in both 2560x2560 eyes. Both eyes
retained their histories during smooth head movement, with zero NGX failures.
Artifacts: `build/foveated-native/19920-offset-debounce`. The original INI and
simulator pose were restored afterward.
