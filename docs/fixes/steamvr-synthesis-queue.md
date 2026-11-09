# SteamVR private synthesis queue

November 6, 2026 test candidate, based on upstream 0.2.2 plus the existing scope,
SDK, recorder and SteamVR handoff fixes. Adapted from V557
`e48d6fcf628943413401e5bb68f53244aef9947a`, `src/layer/openxr_layer.cpp`;
SynthesisQueue retains the LGPL-3.0 attribution. This is a local test build.

## Purpose and scope

The preceding NR stress run dropped from roughly 87 to 49 compositor-estimated
FPS while native generation GPU span stayed near 4.2 ms. Generated display-copy
fences were ready at CPU handoff 96% of the time without NR and 41% with NR.
This supports investigating queue scheduling; it does not prove every pending
fence missed a compositor deadline. See the archived 11:11:32 run for windows.

NVIDIA optical flow plus FidelityFX, and pure FidelityFX, now use a private HIGH
priority DIRECT queue on SteamVR. Native DLSS-G and VDXR retain the shared queue.
The existing handoff-v2 pacing remains. Creation failure safely falls back to
the shared queue and records the reason. No settings are changed for deployment.

This is not the full V557 backend: capture, XR display copies and the runtime
binding still use the application queue. Queue priority is a scheduling request,
not reserved GPU capacity; headset performance and latency require measurement.

## Synchronization and ownership

1. Capture publishes the exact successfully signaled color-copy fence value
   alongside the captured images. The generator waits on that receipt, rather
   than waiting on unrelated game work subsequently queued. Missing receipts
   fall back to a producer signal enqueued before the output join.
2. Synthesis waits for depth/motion input receipts and the preceding output
   consumer fence before reusing history or output storage.
3. The application queue waits for the successfully signaled synthesis fence
   before copying generated images. Priming and partial dispatch paths join too;
   a failed Signal value is never used for the output wait.
4. Each in-flight generation slot retains capture pins and input leases until
   its generation fence completes. Capture diagnostics run on the execution
   queue after the same input waits.

These are GPU waits, with no new per-frame CPU fence wait. Queue identity checks
compare the producer queue, preventing accidental generator recreation per frame.
At shutdown, after the XR owner stops, private generation drains for at most two
seconds while the capture owner exists. Device/fence failure retains unfinished
resources and pins rather than freeing GPU-owned memory or calling a dead owner.

## Validation and diagnostics

MSVC Release built successfully. Eight Windows tests passed, including an actual
D3D12 HIGH queue with the debug layer: 24 delayed-producer input/synthesis/consumer
cycles, exact and fallback input receipts, output reuse and shared-queue bypass.
Production Generator tests passed for private NVIDIA, private FidelityFX and
shared NVIDIA, with stereo midpoint/color checks, input retirement, timing on/off,
and delayed-consumer shutdown. Four Python analyzer fixtures passed.

The startup log records `[framegen-queue]` producer/synthesis pointers, priority,
creation HRESULT and backend. Recorder header is `upstream022-steamvr-synthesis-v1`;
new counters are `synthesis_private`, `synthesis_shared`, `synthesis_queue_fallback`,
`synthesis_input_fallback`, and `synthesis_sync_failed`. Existing compositor loss,
GPU-copy readiness and source-age metrics remain. Compositor-estimated FPS is not
an exact unique-frame or photon count.

For the headset test, use the current built-in NVIDIA framegen with adaptive
pacing and flight recording enabled, external OFXR off. Use normal gameplay with
NR off, then on, and exit normally. Compare ready-at-handoff, compositor losses,
source age and perceived smoothness; do not judge by submission rate alone.

Provenance: `synthesis-provenance.json`; deployment receipt:
`synthesis-deployment.json`; rollback DLL and settings snapshot:
`backups/before-synthesis-*`. Headset benefit remains unvalidated.
