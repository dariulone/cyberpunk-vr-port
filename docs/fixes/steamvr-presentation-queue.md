# SteamVR presentation queue — November 6, 2026

This candidate completes the queue-routing portion following synthesis-v1.
Base upstream0.2.2, corrected2.5 SDK, scope RR fixes, physical refresh correction,
handoff-v2 pacing and private synthesis are retained.

## Motivation and implementation

Synthesis-v1 did generate on a HIGH priority private queue, but it joined output
back to the game queue; eye copies and the runtime binding remained there too.
The latest test had a44FPS estimated-delivery interval, extensive re-priming and
pending display-copy fences. Better late segments may have followed RT disabling;
they do not establish a speedup. See flight-run-20261106-113958/analysis.md.

For SteamVR native D3D12 sessions, the port now creates a separate normal-priority
DIRECT queue (matching V557's binding queue choice), with the same NodeMask.
The session's graphics binding receives this queue. Projection copies and the
optional depth layer execute on it. NVIDIA/FFX synthesis joins its completion
fence directly to this queue, so subsequent game rendering no longer waits on
that output just to hand it back to the compositor. DLSS-G synthesis itself stays
on its original queue but its output receipt joins the presentation queue.

Real captures wait on their exact successful capture receipts. Pending real B
uses B's receipt, not newer C's. Generation retains capture pins, and submitted
image pins still retire against the submit fence (now signaled by presentation).
Depth conversion accepts a different queue only on this explicit private path,
waits already-submitted depth receipts, and retains input leases through submit.
Disarmed frame-capture diagnostics also follow the selected consumer queue.

HUD, interaction/basilisk/surveillance/hit-marker quads, F10 and framegen overlay
retain their existing game-queue rendering. After a successful swapchain wait,
a runtime-queue signal and application-queue wait propagate image availability.
Before release, an application signal and runtime wait propagate the image's
actual writes. Values are allocated/signaled under a mutex; no wait references
an unsignaled future value. Joining at release honors runtimes that enqueue work
during release as well as during EndFrame. No later arbitrary game-queue tail
is joined at EndFrame. Repeated images keep their established dependencies.

Color acquisition now retains timed-out acquisitions for retry, like depth.
Only successful waits permit writes/release. Existing200ms color wait budget is
unchanged. Partial projection-copy command lists submit BEFORE cleanup releases,
so a released image has no application writes still waiting for CPU submission.

No per-frame CPU fence wait is added. Private queue drain is bounded to2s during
shutdown; unfinished queue/fence ownership is retained on failure. Queue remains
alive through overlay and session destruction. Creation failure falls back to
the application queue. External OFXR module presence bypasses private binding.
VDXR keeps the original queue; SteamVR/Steam Link use the same runtime gate.

Third-party layers remain responsible for their own graphics synchronization.
The native optical-flow/FFX algorithms, quality, RT/RR, NR, resolution and pacing
settings are not changed. This is not the full V557 presenter/backend import.

## Validation

Nine Windows tests passed, including actual GPU three-queue operation for24cycles:
display completes while the next game rendering is deliberately blocked;
HUD/reacquire/release joins, fallback capture receipts, shared-path bypass,
shutdown, wait failures/timeouts, and retry of the same acquired image.
D3D12 debug-layer errors are rejected by the fixtures.

Production NVIDIA optical flow and FidelityFX generator tests passed with output
consumed on the separate presentation queue (both-eye midpoint/color checks,
input/capture lease retirement and delayed-consumer shutdown). Actual stereo
depth conversion preserves per-eye0.25/0.5 pixels on the private queue; the default
shared path still rejects an unexpected foreign queue. Four analyzer fixtures
pass. MSVC Release built. Existing scope/source hashes unchanged.

Headset behavior and performance remain unvalidated. No numerical speedup or
unique presented-frame count is claimed from the GPU tests.

## Recorder and test

Header: upstream022-steamvr-presentation-v1. Initialization log:
[presentation-queue] application=... runtime=... private=1 create=0x00000000 external=0.
Counters: presentation_copy, presentation_application_wait,
presentation_runtime_wait, presentation_sync_failed, presentation_ready/pending.
The ready/pending probe marks the runtime queue AFTER all port-owned layer
releases and samples after the handoff pacing wait. It excludes work injected
later by third-party xrEndFrame hooks and is not a photon/deadline measurement.
Existing eye-copy readiness, compositor history and source age remain available.

Use built-in NVIDIA framegen, adaptive pacing and flight recording ON, external
OFXR OFF. Set RT ON and keep RT/RR settings fixed during this run. Test NR OFF,
then NR ON during natural gameplay; verify the HUD/F10/F11 menus, then exit.
This keeps rendering load interpretable; the preceding run ended with RT OFF.

Source hashes and DLL: presentation-provenance.json. Deployment receipt:
presentation-deployment.json. Original DLL/settings snapshot:
backups/before-presentation-*. Only the port DLL is deployed.

## Sources and license

Queue joins adapted from local OFXR V557 e48d6fcf628943413401e5bb68f53244aef9947a,
src/layer/openxr_layer.cpp (binding queue, application release and reacquire).
Derived component retains LGPL-3.0: licenses/OFXR-LGPL-3.0.txt.
The OpenXR D3D12 image-state contract requires availability/writes to be ordered
on the queue in the session binding:
https://registry.khronos.org/OpenXR/specs/1.1/man/html/XR_KHR_D3D12_enable-swapchain-image-state.html
