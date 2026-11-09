# Bike windscreen ghosting: capture and V557 reference

The user reports cleaner bike windscreens with the older V557-based fork. This
is not evidence of a regression in the latest body update. Current PID13852 used
OFA50% throughout with no backend fallback or generation errors; Medium changed
to Slow at406.4seconds. Current interpolation uses FidelityFX plus engine depth
and motion; V557 uses pose-compensated optical flow and custom confidence-aware
composition. The difference is a lead, not a proven cause.

## Capture in game

With NVIDIA OFA framegen enabled, sit on the affected bike and open F10 > FRAMEGEN.
Click **Capture ghosting images (10s delay)**, close F10 and ride with the
windscreen visible. Continue for15seconds, then reopen F10 to check for **Saved**.
The capture pauses while F10 is visible. It expires after60seconds beyond the
countdown if it cannot collect consecutive frames; it does not auto-retry.

Only the explicit button arms image readback. Flight recording is independent.
This captures3 real frames, both eyes, with engine depth/MV and eye poses; it
may cause a brief hitch and uses several hundred MB at headset resolution.
Do not assess performance during the capture. Files go to the plugin's
`log/framegen-capture/<timestamp>-<tick>/`, not bin/x64. Cancel does not free GPU
resources still in use; an already writing capture finishes. A readback timeout
retains pending copies rather than releasing allocations under the GPU.

The runtime DLL changes add capture controls/metadata only; no synthesis math,
quality setting, pacing, haptics or input changes. The new schema records quality,
flow scale and QPC frequency. Existing debug-export capture remains available.

## Offline comparison

`tools/framegen_replay` links the actual current Generator and the existing
V557 static synthesis library. It creates a standalone D3D12 device and opens no
OpenXR session; it does not install layers or change the game. It compares frames
0/2 against the real middle frame1, with rotations mapped into that middle eye.
V557 reference uses NVIDIA, half scale, captured quality, hybrid/bidirectional off,
and no engine guides, matching the older optical-flow-only configuration.

Build with CMake parameters OPENXR_INCLUDE, FFX_SDK, V557_SOURCE and V557_BUILD.
The latter must point to matching V557 sources and Release static libraries
(xrfg_d3d12_history, xrfg_core, xrfg_bridge_flight_logger). Local reference is
OFXR-Bridge/build/cyberpunk-v557-backend and its build-readiness-tests output.
Run `framegen_replay.exe <capture directory> <output directory>` with game closed.
Outputs include both full-eye PNGs, a real-frame reference, and numerical errors.
DLSS-G is skipped if the comparison tool was built without its SDK; this test
compares OFA/FidelityFX/V557. Reference libraries are linked only into the tool.

Limits: this is a wider two-real-frame interval with composed game motion vectors,
not an exact replay of the original in-game generated image. Unequal source
spacing, transparency, animation and pose differences limit whole-image PSNR.
Inspect the windscreen region and both eyes, not just an aggregate score. This
isolates reconstruction from scheduling and cannot measure headset smoothness.
No quality improvement is established until actual bike captures are compared.

## Validation

Release DLL build succeeded. Actual FrameCapture WARP test covers countdown,
menu exclusion, cancellation/rearm, duplicate/gap handling, all18 stereo files,
sizes and replay metadata. Offline hardware replay is tested with distinct-eye
moving-object inputs; this is harness validation, not the reported windscreen.

Hardware fixture replay completed for both eyes through native OFA/FidelityFX
and actual V557. V557 did not win the synthetic rectangle metric; this is not
a windscreen result and no algorithm change was chosen from it. Native tool
checks run under D3D12 validation. Local reference provenance hashes are archived
in integration-notes/bike-ghosting-20261109/reference-provenance.json. V557 source
HEAD is e750741 with later local capture-readiness instrumentation.

## November9 real bike capture and replay correction

The first captured bike comparison reproduced a doubled windscreen edge in both
eyes with the current reconstruction and a cleaner edge with V557. Aggregate
image error does not favour V557 everywhere. Initial replay was biased by FFX's
first10frames forcing game-vector blending; the replay now primes11 stationary
A frames with zero vectors before evaluating A->B. The edge difference remains.
This fabricated warmup is not historical context from the game. Capture intervals
72.2/32.5ms were unequal, so reference frame1 is not an exact midpoint. Do not
report whole-image/ROI metrics as a measured percentage quality improvement.
No production synthesis algorithm changes follow from this harness correction.
