# SteamVR synthesis consumer overlap, November 7, 2026

## Baseline and scope

User accepted RT+NR ON as the ongoing baseline. Stop requesting an RT-off/on/off
sequence. RR remains OFF/deferred. Native NVIDIA framegen, adaptive pacing ON,
SteamVR PSVR2; prior real-recovery DLL8912fe847196d73a37de412e9b05cfd46d16579db1259f1f312f57b92fd89c9a.
Latest natural gameplay estimate73.73FPS, early dip50.43FPS, final30s79.24FPS;
generation total15.40ms, queue7.06ms, previous consumer pending174/246 probes.
These overlapping pending counts do not prove the consumer's individual cost.

## Dependency audit

Presenter exposes only Eye::output as generated pixels to the runtime. Internal
packed color, NVIDIA flow buffers and SDK history are not display resources.
All previous-generation finish work is ordered before new packing on the same
private synthesis queue. That finish is itself behind previous NVIDIA OFA work.
Therefore new packing/OFA does not require previous display consumption to finish.
The interpolation finish list does require the previous consumer receipt before
it may overwrite Eye::output. It retains that exact fence/value wait.

For private NVIDIA synthesis with optical flow only:

1. Wait for captured color and both guide receipts, as before.
2. Record/submit packing; schedule NVIDIA optical flow and its queue waits.
3. Enqueue previous-consumer wait before executing the finish list.
4. Resolve flow, prepare inputs, interpolate, signal final completion and join
   output to presentation, as before.

No fence is removed. No added GPU images/lists/query slots/CPU waits. Guide waits
remain early because CaptureFrame diagnostics may read them. Captured color is
pinned by its lease; work retirement uses the final submitted receipt, including
partial failure handling. Reset/destruction still waits for consumer and GPU
completion. Existing error paths retain allocations after unknown GPU failure.
Pure FidelityFX, no-optical-flow, native DLSS-G and shared/VDXR paths retain early
consumer ordering. Pacing policy and phase recovery are unchanged.

## GPU proof and limits

Added compile-time test-only receipt access (CVR_FRAMEGEN_TEST_RECEIPTS), absent
from the game build. Real NVIDIA/FFX production generator test deliberately queues
a blocked previous display receipt. Before correction it fails 'packing was
unnecessarily blocked by previous display consumer'. After correction packing
completes, output completion remains blocked, and independently read pixels from
both eye outputs equal the preceding frame exactly. Capture lease remains alive,
Reset is refused while output is owned. Releasing the gate completes generation;
both-eye midpoint pixel/centroid checks and input/color retirement pass. Existing
delayed-capture test still proves capture ordering; shared and pure FFX tests pass.
D3D12 error/corruption messages fail the GPU fixture.

This proves safe overlap under an induced dependency. It does not prove a headset
FPS gain. If capture/guide readiness dominates, benefit may be small. Packing can
move earlier without total generation latency improving; compare enqueue-to-end,
source age and compositor delivery, not just enqueue-to-start.

## Recorder and deployment

Header upstream022-steamvr-consumer-overlap-v1. synthesis_consumer_deferred counts
pairs whose consumer wait is deferred; synthesis_consumer_late_pending samples
whether that receipt is still pending when the CPU enqueues the late wait. Neither
is GPU wait duration. Existing sampled generation_consumer_pending is at initial
submit. generation_queue_inputs_pending retains its any-prerequisite cohort
(including consumer even though now later); do not attribute start delay from it.

See consumer-overlap-provenance.json and consumer-overlap-deployment.json for
validated hashes, checks, backup and actual deployment. This candidate is based
on real-recovery-v1; rejected HUD experiment stays absent, scope fixes preserved.
Test natural movement/riding/combat with RT+NR ON throughout, RR OFF, SteamVR
native NVIDIA framegen and adaptive pacing ON. Recorder remains enabled. Preserve
resolution/graphics; exit normally after the run. No repeated scene choreography.
