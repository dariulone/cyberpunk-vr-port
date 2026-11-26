# Native frame-generation checks

Runtime counters and exported JSON require launcher DEBUG or the individual
`CyberpunkVR_RuntimeDiagnostics` gate. `live_probe.py` reports the gate state;
`motion_probe.py` refuses to run while it is disabled. The `debug_gate` case checks
that FPS metrics and framegen remain enabled independently of diagnostic exports.

Build the pinned FidelityFX SDK via the main project first, then:

```powershell
cmake -S tools/framegen_tests -B build/framegen-tests -A x64
cmake --build build/framegen-tests --config Release
ctest --test-dir build/framegen-tests -C Release --output-on-failure
build/framegen-tests/Release/framegen_tests.exe gpu_nvidia
build/framegen-tests/Release/framegen_tests.exe gpu_nvidia_flow
build/framegen-tests/Release/framegen_tests.exe gpu_nvidia_half_motion
```

The last three commands require NVIDIA OFA hardware and exercise it directly. The
other GPU checks use WARP. The moving-box fixture supplies real synthetic motion
vectors/depth for two different eye images. It checks the interpolated centroid,
eye isolation, initial reset, and refusal to free a consumer-owned output.
The D3D12 debug layer's errors fail the tests. The NVIDIA case caught an actual
teardown crash: registered buffers must be released before destroying the OFA
context. The original dump/log are kept under `build/framegen-native`.

Other cases cover native frame gaps and recenter/cut rejection, midpoint/endpoint
ordering when a newer frame arrives or generation is disabled, settings round
trips, unique-frame vs repeat counters, and the stats overlay's single BOTH-eye
layer, zero-timeout acquire/wait ownership and limited refresh rate.

These tests do not validate the game's resource pairing or physical-headset
performance. Those require the freshly deployed plugin, a matching PDB and live
input-stage/report counters, followed by the user's visual check. Never use
RenderDoc replay while the game is running.

The NVIDIA path uses OFA plus FidelityFX Frame Interpolation. Its experimental
custom interpolator was removed after the user found severe gameplay ghosting;
small fixture passes had not established real-scene image quality.
`gpu_nvidia` tests engine MV/depth at moving occlusion edges;
`gpu_nvidia_flow` tests visible motion on a plane with zero engine motion vectors,
as happens with some shading effects. Both report image error against the known
midpoint as well as per-eye centroids. The corresponding FidelityFX cases use
the complete AMD pipeline. GPU fixtures also toggle telemetry off/on while
generation keeps running and verify that GPU timing resumes. These small fixtures establish correctness only;
their costs and errors are not an in-game quality/performance ranking.

`pacing` migrates16 Present calls between workers. The target150ms run took6ms
with a thread-local deadline; the shared render-stream schedule fixes that.

`metrics_disabled` verifies that turning off the FPS overlay stops counters and
hardware monitoring even while frame generation remains enabled. The XR overlay
test covers disabling with an acquired image that is not ready yet: it becomes
invisible immediately and retires its resources after a successful wait/release.

`adaptive_pacing` verifies that repeated missed adjacent frames lower the real
frame target, isolated spikes do not, the backoff is bounded, and the target
gradually returns to half refresh after healthy delivery. This does not change
the game's graphics settings or its maximum-FPS setting.

`gpu_fidelityfx_half_motion` and `gpu_nvidia_half_motion` cover full-resolution
depth with half-resolution motion, then a live motion-size change. All generation
fixtures also verify completed input leases retire through `Generator::Poll`
without waiting for the next generation. The planar-depth case exercises the
production copier with different depth/stencil plane states.

Native TAA input capture is selected by the exact ApplyTXAA consumer and logical
resource identities, not by texture size. Its state lookup and per-eye frame
association require live validation against the supported game executable.

## Measuring a moving hand

Mark a small textured region on the hand in real frame 1, independently in each
eye. Coordinates are full eye-output pixels from the top left. Repeat on a
static landmark. For the 2026-11-18 fixed-head scene-plane triplet (frames
15752–15754), the left and right hand boxes used below are on the grip.

```powershell
$fg = './build/scene-hand-tests/Release/framegen_tests.exe'
$scene = (Get-Content -LiteralPath build/physical-melee/fg-scene-baseline.txt -Raw).Trim()
& $fg hand_motion $scene 0 1168 1399 115 114
& $fg hand_motion $scene 1 353 1399 115 114
& $fg hand_motion $scene 0 1130 825 130 100
```

`hand_motion` runs on the CPU. It searches four luma resolutions to match
9×9 patches in both directions. It reports the accepted sample count after a
two-pixel round-trip check, a contrast check and a photometric error check.
The engine measurement samples the current frame's XY motion at the jittered
render location and converts it to display pixels using its recorded eye scale.
EPE is Euclidean error against the independently observed current-to-previous
displacement. The output includes median, p90, the fraction above 3 px,
median device depth, and counterfactual errors for half horizontal scale and
zero motion. Occluded, ambiguous and textureless points can drop out. Mark
the same surface again when equipment, pose or plane changes.

| Frames | Eye and frame-1 box (x y w h) | Matches on each interval | Median EPE (px) | Above 3 px |
|---|---|---|---|---|
| Scene 15752–15754 | L 1168 1399 115 114 | 48, 48 | 49.93, 119.57 | 100%, 100% |
| Scene 15752–15754 | R 353 1399 115 114 | 56, 49 | 48.80, 119.64 | 100%, 100% |
| Native 16990–16992 | L 1268 1455 115 114 | 107, 116 | 0.49, 0.43 | 0.9%, 1.7% |
| Native 16990–16992 | R 476 1455 115 114 | 111, 122 | 0.49, 0.46 | 0.0%, 0.8% |

For offline GPU synthesis after coordination, supply the same two boxes and
the run's optical-flow percentage. The following example assumes 50%.

```powershell
& $fg hand_midpoints $scene build/scene-hand-tests/scene-midpoints 50 1168 1399 115 114 353 1399 115 114
```

This feeds the production NVIDIA OFA + FidelityFX generator both eye inputs
at their recorded sizes and quality 2. A stationary frame 0 primes its
history, followed by the original 0→1 and 1→2 intervals. Each output crop is
saved in the specified workspace directory. Timed captures use QPC deltas
with the analysis host's frequency (run on the capture machine). Older
untimed captures use 22.22 ms. On each accepted track the test
compares a 5×5 luma patch at the predicted halfway location with corresponding
endpoint detail, and scores an unwarped endpoint blend at the same location.
The score assumes approximately linear screen motion within one interval.
It does not require frame 1 to lie halfway between frames 0 and 2, which the
scene capture violates. It measures generated color before headset submission.

A producer fix should first recover nonzero vectors on the moving grip and
bring the two eyes' EPE/outlier rates toward the native-plane measurements.
Recheck the stationary landmark, scene depth/occlusion and actual midpoint
crops on the same trajectory. The pass investigation starts at
`GBufferEarly_Velocity_Weapon`, `GBufferVelocity_Solid` and
`GBufferVelocity_Discard`. At the tracked surface compare the hand color/depth
draw with the velocity attachment's pixel history, depth/stencil test, bound
previous skin/instance transform and both eye camera blocks. An upscaler-input
triplet records the final vectors after all of these passes.

In the 2.5 full graph builder at RVA `0x2276E60`, the early pass requests
the regular and discarded velocity stages in bucket 2 (flags 33). The later
solid and discard velocity passes request those stages in bucket 0 (flags
17). The pass driver at `0x33DDA4` looks up the bucket before selecting the
view parameter block. The existing scene option changes that block at
`0x33E0B8` and changes the plane byte used by one draw-list producer at
`0x4A0983`. An additional producer at `0x32E916` reads proxy+0x9D and
passes it to the same bucket allocator. These are distinct sites to identify
in the hand draw. Current pose solves and same-batch cache replays in
`AnimPose.cpp` operate on the current player bone buffer. The retained native
player-skin trace in `docs/cp2077-native-chain-index.md` follows mapped
model transforms through 3×4 matrices to the upload resource. Attribution of
the previous palette at the scene hand draw requires the producing draw.

### Scene velocity overwrite, 2026-11-18

The near-surface depth in `hand-motion_frame3076.rdc` matches native source
frame **3077** in the 21:31:40 triplet, in both eyes. The other RDC is outside
the triplet. At left velocity texel (1076,940), draw 43664 wrote
(-0.0024204,+0.0027218) into RG16F target 133014 at device depth 0.04532.
These are native side-by-side UV units. Multiplication by (5296,2864) gives
(-12.82,+7.80) eye-display pixels. The skinned VS reads a bone buffer at t10.
The velocity is already accurate at the output of this draw.

Event 66106 then replaced the vector with (0,0). Its full-screen pixel shader
(DXIL shader hash `64b2ada20da33a67b6ae05c718a58dd0`) samples scene depth
and discards depths above 0.9. For other depths it writes the camera's
reprojection, which is zero with the fixed head. Both eyes' input copies
read the resulting target at events 87058/87063. The ordinary scene geometry
velocity draws follow the two camera initialization triangles.

| Native 3077 hand region | Accepted tracks | Median / p90 EPE before initialization | Median / p90 EPE at input copy |
|---|---:|---:|---:|
| Left eye | 102 | 0.65 / 1.51 px | 15.26 / 15.26 px |
| Right eye | 100 | 0.67 / 1.34 px | 15.26 / 15.65 px |

Every sampled track had identical early and visible scene depth. The test
candidate adds the first-person material bucket to the two ordinary scene
velocity passes on headset views with scene routing applied. The engine draws
it against the populated scene depth, using the native per-eye scene cameras.

**Single-binary live check.** Use the fixed head, fixed wrist and diagonal
sweep. While keeping plane 1, backend 1, quality 2 and flow scale 50, compare
the exported 32-bit `CyberpunkVR_SceneObjectVelocity` at 0 and 1 (default).
With runtime diagnostics enabled the two 64-bit
`CyberpunkVR_SceneVelocityPasses` counters count augmented solid/discard pass
invocations. An invocation can have an empty material bucket. Save an
ordinary three-frame framegen input dump during each sweep. Avoid capture-induced
timing gaps when scoring generated output. Also sweep partly behind a fixed
edge to check depth rejection on the occluded section.

In the existing 21:31:40 triplet the frame-1 grip boxes (x y w h) are
`1795 1550 125 135` left and `992 1550 125 135` right. Remark the boxes on
the corresponding surface in each new triplet. Aim for median EPE under
2 display pixels and p90 under 3 in **each** eye on the moving grip, with
the stationary landmark still near zero. Use `hand_midpoints` on the
ordinary-timing dumps to compare generated patch error and inspect the
before/mid/after crops. Capture-time QPC gaps in the RDC triplet exceed the
presenter's normal 250 ms limit.

### Gameplay A/B, root-fg01

Same DLL, fixed camera, scene plane 1, backend 1, quality 2, flow 50.
The native input triplets are 21:56:34 (correction off) and 21:56:38 (on).
Measure the following boxes on frame 1 of each triplet, in eye-display
pixels. Grip boxes are 118×116. Left/right origins are (1250,1438) /
(455,1438) with the correction off and (1175,1376) / (346,1376) with it on.

| Correction | Frame interval | Grip median EPE, L / R (px) | Grip p90 EPE, L / R (px) | Generated patch MAE, L / R (luma / 255) | Endpoint blend, L / R |
|---|---|---:|---:|---:|---:|
| Off | 3099→3100 | 11.40 / 11.70 | 12.37 / 13.42 | 19.96 / 17.42 | 20.00 / 17.45 |
| Off | 3100→3101 | 54.04 / 54.04 | 55.44 / 55.44 | 29.25 / 25.64 | 29.30 / 25.70 |
| On | 3283→3284 | 0.55 / 0.45 | 1.29 / 1.02 | 2.32 / 2.60 | 38.21 / 32.00 |
| On | 3284→3285 | 0.60 / 0.57 | 1.51 / 1.50 | 2.21 / 2.17 | 35.90 / 30.46 |

The accepted gameplay tracks above 3 px fell from 266/270 to 9/335.
The left grip in frame 3285 moved (-142,-27) px back to frame 3284 and
the captured engine vector was (-142.0,-27.4) px (component medians).
The stationary background had zero median and p90 EPE in both eyes in
both runs. Sleeve median EPE was 0.38–0.52 px with the correction on.

The GPU scores use the production generator and the track-based midpoint
estimate described above. The sweeps had different displacements, so compare
each generated score with its own endpoint blend. On the corrected input the
reduction was 92–94% in all four eye/interval measurements. The runtime
diagnostics gate was off. This comparison uses the consumed motion textures.

The left-eye grip crops show the two source images and the generated frame between them in each run. [In-game source and generated frames](../../docs/evidence/vr-feel-20261118/framegen-hand-midpoint.png).
