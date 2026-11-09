# Sparse GPU completion versus handoff timing — November 7, 2026

## Purpose and preserved baseline

Producer-gate-v1 (79b478b4) improved delivery. Producer-credit-v1 was rejected and
rolled back: barely higher real throughput, lower compositor estimate despite
younger captures. Existing readiness checks only say pending/ready at handoff;
they cannot establish when pending work completed. This diagnostic keeps the
producer gate, submit schedule, generation algorithms, settings and queue priorities.
It measures the missing completion-versus-runtime-boundary relation before another
pacing policy change. RR crash work stays deferred.

## Sample ownership and GPU work

On every31st eligible SteamVR private-queue FG/adaptive/recorder cycle, enqueue a
tiny timestamp-only command list AFTER all port-owned eye/HUD/F10/metrics layer
releases and BEFORE the existing whole-presentation fence marker/pacing hold.
The calibrated end timestamp follows this queue's prior work/dependencies. It
excludes commands that the runtime enqueues later during xrEndFrame. Query resolve
and fence signal follow the timestamp, so it is a completion marker with a short
tail, not proof of swapchain consumption or scanout. About2.9 probes/s at90Hz.
This adds one small GPU submission/two timestamp queries per sample, not zero
instrumentation overhead. No queue wait, CPU wait or dependency edge is added.

Four fixed slots own allocators/lists/query heaps/readbacks; never reset before
GPU completion and sealed XR metadata. A full pool skips. Failed submissions
quarantine their slot. Queue owner drains before destruction, or retains the
entire route/pool if draining fails. No image textures are allocated. Existing
CaptureTiming/GpuCorrelation reject invalid calibration; no cross-queue GPU tick
comparisons. Clock calibration is recorded immediately before sample enqueue.
Readback is polled later only after fence completion, on the same XR owner.

## CPU/runtime pairing and limits

The exact existing GetFrameTimeRemaining read is bracketed with QPC reads for the
sample, without another OpenVR query. Save midpoint/width, period, selected pacing
margin, real/synthetic/repeat kind, actual xrEndFrame entry/return and accepted
status. Width>0.25ms, invalid time/period, rejected XR calls or calibration failures
are not classified as successful samples. Existing scheduling feedback unchanged.

OpenVR's shipped header documents early rollover: time remaining can advance to
the next frame before zero. Therefore `probe + remaining` is a RUNTIME-REPORTED
frame boundary estimate, not an exact physical-vsync/photon deadline. Values over
one physical period are separately marked future boundaries. The target is that
estimate minus the EXISTING margin: a pacing setpoint, NOT compositor cutoff.
The marker's GPU time can be before/after CPU handoff, setpoint, and boundary;
+/-0.25ms near classifications is ambiguous. Crossing one estimate is not proof
of a dropped frame. Actual compositor aggregate delivery is still needed.

Counts classify GPU-versus-handoff and boundary, handoff-versus-target, per-kind
late-GPU cases, and GPU-late/CPU-early versus CPU-late/GPU-early. Positive lag/lead
magnitudes have separate timings; raw paired samples retain signed interpretation.

A64-record fixed buffer passes paired samples to the recorder worker. Renderer
uses try_lock and drops/counts if busy/full; it never waits for logging and does
no JSON/file IO. The worker writes `display_probes` arrays with paired QPC values
inside existing one-second records. Accepted samples only; dropped/skipped/invalid
counts retained. Flight recording OFF, non-private queues/VDXR, or inactive FG/
adaptive pacing create no new probes. A few outstanding probes can be collected
later. The overall recorder's existing16MiB limit remains.

## Validation

Twelve flight/GPU Windows tests plus production HUD pass (13 total), and Release
build succeeds. New hardware D3D12 test blocks the queue for25ms: GPU lateness is
recognized while CPU handoff is timely; separate test completes GPU work first,
then delays handoff and correctly identifies CPU lateness. Four-slot exhaustion
skips, pending/unsealed data stays unread, all slots reuse safely for16 more
samples, D3D12 errors fail. Pure classifier tests include rollover/failure/invalid
read duration. Existing pacer test exercises the production paired runtime read;
recorder test verifies bounded64-record overflow/drop accounting and final flush.
Python analyzer checks per-kind late GPU/late CPU/future-boundary classification
and reads real recorder fixture output. Scope-source hashes and original producer
pacing sources match the good baseline. No headset conclusions yet.

## Next run / analysis

RT+NR ON, RR OFF, native NVIDIA FG + adaptive pacing ON, SteamVR; normal play and
exit. No stationary scene or extra toggle sequence. Recorder remains enabled.
Use tools/flight_recorder/display_deadlines.py LOG --from-seconds START
--to-seconds END after identifying gameplay, alongside existing delivery analyzer.
The script separates real/synthetic/repeat and future/within-period boundaries.
Ready-before-boundary but badly phased handoff directs work to release/submit
alignment; predominantly late GPU completion directs work to GPU dependency/
critical path. Neither alone proves actual scanout or justifies removing fences.
Check diagnostic overhead and compare with prior gate baseline; do not claim this
build itself is a performance fix. Rejected credit code remains excluded.
