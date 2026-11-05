# Shader audit for engine stereo with asymmetric eye windows, game 2.5

Task: audit every 2.5 pixel and compute shader for assumptions that break when the frame is side by side
(3072x1661, one 1536-wide half per eye) and each eye has its own asymmetric projection window. Trigger: distant
cars lighter in one eye (`render-doc-capture/carsLighterInOneEye.rdc`, ManualSave-486).

## What breaks under side by side

Each eye is drawn into its half with its own camera block (b1 CameraShaderConsts, 852 B: matrices rows 0-35, eye
position 36, forward 41, view size 47.xy = 1536x1661, jitter 51, camera kind 53). b0 GlobalShaderConsts still
describes the WHOLE frame (rows 2-4: render size and reciprocals; row 32.x = two-halves flag). A shader goes wrong
when it mixes the two:

| Category | Pattern | Effect |
|---|---|---|
| frame_eye | pixel position scaled by b0 rows 2-4 (whole frame) and then fed to b1 (eye) matrices or vectors | right eye pixels land at x = 1..2 of an NDC made for one view: wrong rays, lighting from the wrong direction |
| tangent | `ndc * constant` without an off-centre term | the eye window's NDC shift is ignored (volumetric fog, SSR had it) |
| forward | b1 row 41 used as the view direction without the eye position | one direction for the whole eye |
| centre | frame uv - 0.5 squared or dotted | distance from the FRAME centre, which is the seam between the eyes |
| view_size / stereo | reads b1 row 47, b1 row 53 or b0 row 32 | the shader is stereo-aware (informational) |

## Tools

- `tools/asym_tests/sbs_audit_scan.py <ll_all> <out.json>`: static scan of the disassembled corpus
  (`build/patchday/shaders-2.5/ll_all/{ps,cs,sps,scs}`) for the categories above. Output
  `build/sbs-audit-20261027/scan.json`.
- `tools/asym_tests/frame_shaders.py <capture.rdc>`: reads a RenderDoc capture as a file (no replay), maps every
  bound pipeline's shader blobs by DXIL hash to the corpus and to our twins (`all_*`, `static_*`, `src/Shaders`).
  Output `build/sbs-audit-20261027/frame_cars.json`.
- `tools/asym_tests/fog_jitter_probe.py`: diagnostic variants of the volumetric-fog twins (`--freeze`,
  `--history W`, `--jitter-scale S`). Measurement tools, never shipped as they are.

## Results

Corpus: 3216 pixel shaders, 617 compute shaders (plus 617 / 134 static-pipeline copies).

| Category | ps | cs | in the captured frame |
|---|---|---|---|
| frame_eye | 1 | 0 | 0 after the fix |
| tangent | 246 | 76 | 10 |
| forward | 105 | 1 | 0 |
| centre | 23 | 78 | 10 |
| stereo-aware (row 53 / row 32) | 1683 | 132 | 92 |

The frame (357 distinct pipelines' shaders, 220 known to the corpus, 17 blobs are ours) was clean apart from ONE
shader of our own: the `view_dir` twin family. It rebuilt the pixel ray from b0's whole-frame reciprocals and
then used the eye's matrices, so the right eye's distant cars got their lighting from a ray outside its window.
Fixed in all 24 `view_dir` twins (`tools/asym_tests/view_dir_shaders.py`, `src/Shaders/view_dir`): the pixel is
taken relative to its own half and scaled by b1 row 47. Verified live on ManualSave-486: both eyes equal.

The remaining in-frame hits were reviewed:

- six pixel shaders with `tangent` on b4[3].x / b4[4].x: b4 is not the camera block, and all six read b1 rows
  36/41/47/51/53 and both stereo markers, i.e. they build their rays from the eye's own block;
- `cs/54a5f0fb56d1de3e` and two static copies with `tangent` on b12 row 21: they read the two-halves flag;
- the `centre` compute hits (nine native, plus our hidden-area twin) were not traced one by one in this pass;
  the capture shows no one-eye difference that leads to them.

No other in-frame shader showed a one-eye defect.

The one native `frame_eye` match in the corpus, `ps/6ca166b598e07835` (not in the capture), reads b0 row 32 and
b1 row 53 next to each of its four whole-frame pixel scalings, i.e. it picks the half itself. Left as it is; it
is the first shader to check if a one-eye defect turns up in a scene it draws. Shaders outside the captured frame are covered by the static scan and
the existing twin families (asym, asym_ao, asym_ssr, asym_shadow, asym_particles, asym_expo, view_dir,
stereo_*). A new scene that shows one-eye differences should be captured and run through `frame_shaders.py`
first: it answers "is this a native shader or one of ours" in seconds, without a replay.

## Volumetric fog that jumps on a still camera (same session)

Measured at night over the crossing (-2413.6, -958.2) in cloudy weather, Medium fog resolution, head still,
12 grabs of the mirror 0.7 s apart, per-pixel max step:

- native fog: the blurred light streaks in the haze under the lit canopy change every grab (region p95 10-12
  luminance levels); everything else that changes is animated content (signs, screens, NPCs, cars);
- our twin with the per-frame jitter frozen (`--freeze`): the streaks stop changing (p95 3), but every point
  light resolves into a round blob: not shippable.

So the jumps are the froxel sample jitter (`frac(COM row 10 - 0.5 + noise[x&31, y&31, z&31])`) that the 0.98
history does not average out where the samples fall in and out of a light's cone. The history itself is sound:
the twin reprojects with the eye's previous matrices and keeps each eye in its own half of the 192x104x64 volume
(96x104 froxels per eye); the noise index repeats every 32 froxels, so both eyes jitter identically and there is
no binocular shimmer. Ultra has smaller froxels, hence almost no jumps.

Canopy band (mean per-pixel max step over 12 grabs, left eye; A/B/A/B against the native shader each time):

| Variant | Band | Note |
|---|---|---|
| native, Medium (0.98) | 3.3-3.7 | |
| native, Ultra | 1.0-1.1 | the reference: "almost gone at Ultra" |
| jitter frozen | 0.7 | point lights become round blobs |
| jitter x0.5 | 3.8-4.1 | no gain |
| history 0.99 | 1.7-2.2 | first fix |
| history 0.995 | 1.0 | Ultra's level, but a long history smears the haze while the view moves and trails moving lights |
| **history by motion, Medium** | **1.05-1.12** | shipped |

Shipped: `asym_fog_shaders.py` `motion_history` in the two scattering twins (601954c7c7c00b49, f60d19af775ca034).
The weight follows how far the reprojection moved the froxel this frame: the history sample's texel against the
froxel's own (b12 row 43 .yzw = the volume size), 0.995 at rest, falling linearly to the native 0.98 at a quarter
texel (a 30 deg/s head turn moves about a third of a texel per frame at 80 fps). At rest Medium now flickers as
little as Ultra; frames grabbed in the middle of a 63 deg/s head sweep look the same as with the native shader.
The cost left: with the head still, a light moving through the haze leaves a trail with a 1.7 s half-life
(native 0.43 s).

## Planar reflections: one eye's water showed the other eye's reflection (2026-10-30)

The screen-space planar reflections (water; render-graph node `sub_14077D450`, trace `cs/5e01aaacc7c617d2`) were not
in the cars capture: they run only with Screen Space Reflections on (`ScreenSpaceReflectionsQuality`, which the game
applies at start, not from the settings system mid-session) and only where a planar surface is in view.

In the side-by-side mode the node draws the planar stage once per eye into that eye's half (viewport x = half width
for the right eye) and then dispatches the trace once per eye with that eye's block (b6 row 40.w = 1 left, 2 right;
two 656-byte uploads per frame, `CyberpunkVR_DebugSsrUploads`) -- but each dispatch covers the WHOLE frame
(groups = ceil(width / 8) both times). The shader folds the thread's x into its half, builds the ray with the
block's camera and samples the half the block names, and it never checks which half the thread is in. So the left
eye's dispatch traced both halves and the right eye's then traced both again: every pixel ended with the right
eye's trace, the left eye's water carrying the right eye's reflection laid out in the right eye's window. With the
asymmetric windows the two layouts are about a quarter of an eye apart; on the Pacifica bay the left eye's water was
dark brown where the right eye's reflected the sky. The SSR node does halve its groups per eye
(`sub_14077A100`: `ceil((width >> 1) / 32)` under feature 98); of the 46 functions that test feature 98, these two
are the only per-eye loops around a compute dispatch.

Fixed in the planar twin (`tools/asym_tests/asym_ssr_shaders.py` `eye_half_exit`, `src/Shaders/asym_ssr`): in the
two-halves mode a thread outside the dispatch's own eye returns before it reads or writes anything, which also
halves the trace's work. Live A/B on the bay, the twins off and on, mean change of the water per eye: left 13.3
(frame noise 3.3), right 3.0 (noise 3.4); with the twin the left eye's water matches the right eye's.
