# Engine stereo: reflections from each eye, not the stereo centre (2026-11-02)

Under engine stereo (2.5, `xr_engine_stereo=1`) reflections looked painted onto surfaces: the same reflected image
at a surface point in both eyes. Every case found so far has one cause: a view direction measured from the engine's
**stereo centre** (half an eye separation, ~3.2 cm, from each eye) instead of from the eye being drawn.

Two sources of the centre:

- **b12 (SharedPixelConsts) row 0**: one position for both eyes, the stereo centre. Shaders form
  `V = normalize(row0 - P)` with P from the eye's own inverse (b12 rows 81-84 / 101-104).
- **gCommonConstants rows 0-3 (RT)**: a clip->world inverse *relative to* the centre (its rays converge at
  +-0.0323 m along right, measured live; rows 8-11, first person, converge at 0 = the eye). Shaders form
  `V = normalize(P_rel)`.

Fix: take the eye. In draws/dispatches made once per eye (b1 row 53.x = 1 / 2) the bound camera block's row 36 is
that eye; for a centre-relative inverse the eye is row k+2 / its w (clip (0, 0, 1, 0), reverse-Z).

## Fixed

| Pass | Shaders | Generator | Folder / switch |
|---|---|---|---|
| Deferred ambient (probe and SSR/RT composite) | 20 static ps | `stereo_ssr_merge_shaders.py` `eye_view` | `stereo_merge\`, `CyberpunkVR_StereoMergePatch` |
| Tiled deferred lights (specular) | 78 static cs | same, `per_eye_dispatch=True` (`LIGHT_PASS`) | same |
| Forward-lit materials (glass, clear coat, ...) | 215 ps + 19 vs from shader_final.cache | same, `--forward` (`stereo_forward_shaders.txt`) | same |
| RT reflections, opaque | 2 DXIL libraries | `stereo_rt_shaders.py` (camera ray **and** the biased incident ray the trace mirrors) | `stereo_rt\`, `CyberpunkVR_StereoRtPatch`, swapped at `CreateStateObject` |
| RT reflections, transparent | 1 library (camera ray only; it mirrors that) | same | same |
| Path tracing (`rgs_reference_main`) | 34 libraries (same two sites as opaque) | same | same |

Headset results: SSR face-on puddles, the metal trim's light highlight, and RT puddles are per eye. A 1.5 m live
shift of the eye position (`rt_cam_shift.py --node 0 --kind -1`) moves the wallpaper's gold sheen, so those passes
draw it; at the real 3.2 cm a rough metal barely changes, as expected. Car glass and paint: confirmed per eye on
2026-11-03 (below).

Note: the first RT twin changed only `normalize(P)`, which feeds just the ray's start bias; a x30 diagnostic changed
nothing in the headset. The trace mirrors `normalize(P - V*bias + N*bias)`, which needed the same correction.

## RT survey (`tools/asym_tests/view_survey.py` over all 68 libraries + the live census)

- Reflections opaque/transparent, path tracing: fixed above.
- ReSTIR GI (13): no view direction. Shadows (14), importance, diffuse: light directions only. Nothing to fix.
- Reflection raygens also take height fog along the reflected ray from the centre (SharedPixelConsts row 0 and
  CameraShaderConsts row 36, the head centre): fixed by `fog_origin`. 2 sites per reflection library, origin =
  centre + gCommonConstants row 2 / w.
- DLSS Ray Reconstruction (feature 1001) ran once over the side-by-side frame with one camera. It is now split per
  eye like DLSS (`EngineStereoDlss.cpp`): its own viewport per eye, the engine's `slDLSSDSetOptions` options at
  half width, and each eye's camera position in the constants and the view matrices.
  Live switches: `CyberpunkVR_EngineStereoRrPerEye` and `CyberpunkVR_EngineStereoRrEyeCamera`.
  Counters: `CyberpunkVR_DebugEngineStereoRr`.
  The first split logs the engine's RR matrices (`per-eye RR:`). Check them for the layout `MoveViewMatrices` expects.
  RR's left eye has to read copies of its halves at (0, 0) like the right eye (`CyberpunkVR_EngineStereoRrCopyLeft`):
  reading its halves in place at offset 0, as DLSS's left eye does, ghosted fog and fire stuck to the screen in the
  left eye. Headset: the ghost went with the copies and came back with the switch at 0. No tag is eOnlyValidNow (a
  snapshot at tag time never fired), so that was not the cause.
- Water: no shader of its own was identified. Its parts are the planar trace (per eye since 0.2.0), the ambient
  probe fallback, forward-lit materials and the RT opaque raygen, all twinned above. Still painted on in RT+RR with
  the RR split on (screenshot 11): reflected buildings at 246.8-248.3 px disparity, the real ones at 245.8, the
  banks at 246-251; a mirror image should sit at or beyond the real buildings.
- Car glass (`326316c50cf8c2e2`): its twin is bound and takes the stereo branch, and its b1 row 36 is each eye's
  own position (diagnostic colours per eye), yet the reflection did not change: it comes from another input. A
  knockout twin (left eye: no t44 probe atlas samples; right eye: the t56 screen buffer's valid test false) tells
  which.
- Live RT nodes (RT reflections + AO + local/global shadow on): the reflection node's 3 compute passes, the
  `RayTracingFilterOutput` denoiser (it rebuilds P only for a world-space mask lookup), the AO and shadow filters,
  the 4 unnamed passes in 0x2284370: no view direction.
- Not running with these settings: ReSTIR GI, path tracing, transparent reflections.

## SSR survey

- **The traces are per eye by construction.** 21 compute shaders share the SSR block layout (b6, 656 B, uploaded per
  eye, row 40.w = eye): the 24/64/128-step and Low traces (`b86f8871454997e6`, `c827b32cf54f25fb`,
  `8661cc86756e2ecc`, `acaedc11005a92d0`), `842ab722bfe403cd` and 16 more permutations. V is the view-space position
  from the block's per-eye projection (rows 4-5, including the eye window's shift) divided by depth, so its origin is
  the projection's apex: the eye that rendered that half. Normals go to view space by rotation only (rows 8-10).
  Measured: an oblique SSR reflection sat at the far sign's disparity (252-253 px), not the road's.
- **Planar trace** (`5e01aaacc7c617d2`): same block and construction; its whole-frame dispatch per eye is fixed by
  `eye_half_exit` (asym_ssr, 2026-10-30).
- **What made SSR look painted on** was not the trace: where rays miss (face-on, looking down) the ambient pass falls
  back to the probe with the centre's V. Fixed (ambient, above).
- **Known, not a perspective bug:** the Low trace maps only its pixel, not its march, to the eye's half (the right
  eye marches through the left half): `engine-stereo-plan.md`.
- **Full-screen ps `005d159bb02ba89e`, `1d476c7f48e86120`, `9ad0420e0ddd85ed`, `946b94470e835342`:** a camera ray
  through b6 rows 28-31; sky/atmosphere-style passes (a direction to infinity), not reflections.
- **Not yet covered:** the SSR node's other passes (resolve / temporal filter, the PrepareFeedbackSSRBuffer pre/post
  nodes, ReflectionProbes 0xDAC960, MarkTransparentReflection 0xE9CE6C). Run with SSR on:
  `rt_dispatch_census.py --pid PID --nodes 0x77A100,0x77D450,0x77E104,0xDD3D24,0xDAC960,0xE9CE6C`, then
  `view_survey.py` on the shaders it lists.

## The car paint (2026-11-03): a mod had replaced the transparent raygen

The last painted-on case was the mirror image in car paint (clear coat) and glass. How it was found:

- **Not the opaque raygens.** Their trace direction is built only from the patched view (backward slice of the main
  `traceRay`: normalize(reflect(V', H)), V' = normalize(Q - eye), H a GGX sample), and a diagnostic that stored the
  ray direction as stripes with a per-eye level showed the eye offset differing at run time with the measured values
  (+-(0.0078, -0.0314)). With their output replaced by stripes, the hood still showed the sharp mirror image.
- **Draw-skip bisect over pixel-shader hashes:** the hood's mirror is forward ps `69a687939469a979` (on the forward
  list, its view-direction twin already bound). A per-eye knockout of its inputs (`forward_knockout.py`: left eye
  without the t44 probe atlas, right eye without the t56 screen reflection) took it out of the right eye: **t56**.
- **t56 is the transparent RT raygen's `reflectionImage`:** `rgs_reflection_transparent_main` stores rgb / 64 and the
  surface depth in w, which 69a6 reads back at its pixel (x 64, depth test). Its twin (7c98) was correct but never
  matched: a library census at `CreateStateObject` showed the game creating **`16a3f056d8ec9845`** instead.
- **The mod is ReFRACT** (Nexus 34858, v1.5.1, `red4ext\plugins\ReFRACT\ReFRACTNative.dll`), a car-glass
  refraction mod. It rewrites `rgs_reflection_transparent_main` at run time in its own `CreateStateObject` hook, which
  runs before ours (sun-shadow traces added, values named `rfSun*`; the bytes exist nowhere on disk). With ReFRACT
  uninstalled the game creates vanilla 7c98, its twin is swapped in (`twin yes` in the log), and in the headset all
  reflections, car paint and glass included, sit behind their surfaces.

`stereo_rt\16a3f056d8ec9845.dxil` (twin of ReFRACT's library) is included and swaps in (`twin yes`), but it was not
checked in the headset, and it only matches ReFRACT 1.5.1 with the settings it was captured under (its menu options
probably build other libraries). Twins keyed by the library's
FNV-1a cannot follow a library another mod rewrites; supporting ReFRACT properly means patching the transparent
raygen's IR at `CreateStateObject` time instead of swapping a prebuilt file.

Also measured on the way:
- The reflection node dispatches 2060 x 2403 (one eye's width), ~5 per frame: each eye traces its own half.
- Live reflection blocks: gCommonConstants rows 0-3 are centre-relative (row 2 / w = +-3.23 cm per eye), rows 16-19 per
  eye, row 56 and rows 20-23 the centre. The denoiser's copy has absolute rows 0-3.
- A per-eye tint of the opaque raygens' output stays per eye on screen: the denoiser does not mix the eyes.

The live probes used here (draw/work skip, the RT block ring and camera shift, the library census) were removed
before committing; `stereo_rt_shaders.py` keeps its `--debug-tint/--debug-stripes/--debug-dirs` builds.

## Open

- SSR looks low resolution in VR even on Insane (per eye is correct). The SSR node dispatches 65x76 / 129x75 groups
  per eye: find whether the engine halves SSR resolution under stereo.
