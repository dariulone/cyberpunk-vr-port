# SteamVR GPU readiness and phase feedback

Candidate: `readiness-v1`, based on tested `presentation-v1`, upstream 0.2.2.
November 6, 2026. SDK and RR/scope fixes preserved.

## Evidence and intent

The 15:55 run improved markedly with the separate runtime presentation queue.
Before NR: 44.02 real + 43.86 generated accepted/s, 89.59 compositor estimate.
With NR: 41.98 real + 41.21 generated accepted/s, 68.99 estimate. Only 14.53%
of whole-port presentation receipts were complete at CPU handoff, and the phase
controller performed 211 whole-period acquisitions in 40.23 seconds. GPU
generation span changed 4.984 to 5.346 ms, excluding pre-execution queue delay.
Stable memory and no sync/generation errors. These natural-play windows are
not a scene-matched benchmark, and submission counts are not displayed FPS.

The phase controller previously interpreted compositor remaining time without
checking whether the current port images had finished rendering. GPU-pending
frames cannot establish whether a CPU handoff phase change is appropriate.
The recordings justify guarding that feedback, not a claim that it caused all
misses or that the guard will restore 90 FPS under NR.

## Change

After all port-owned eye/depth and overlay releases, insert the same runtime
queue fence marker used in presentation-v1 diagnostics. Poll its completion
after the existing pacing hold and compositor timing read. It is also active
with the flight recorder disabled when adaptive FG pacing is enabled.

Known pending work suppresses phase corrections, including whole-period jumps,
while preserving the absolute refresh cadence and existing overrun handling.
Any pending real/synthetic/repeated frame arms recovery. Two ready real frames
with valid compositor feedback restore steering; synthetic frames alone cannot
satisfy recovery. A new pending frame restarts recovery. Unknown readiness
preserves the legacy/shared-queue path. No new CPU wait, GPU wait, extra image
buffer or rendering-quality change is introduced.

This only changes the existing SteamVR adaptive pacing path with a private
presentation queue. VDXR does not enter the pacer; external OFXR bypasses the
private binding and has no readiness receipt. Disabling adaptive pacing still
disables this controller. Existing generation, capture and overlay ownership
joins are preserved.

## Additional recorder measurements

Header: `upstream022-steamvr-readiness-v1`.

- `phase_gpu_pending`: cycles whose readiness probe prevented steering.
- `phase_gpu_recovery`: ready real frames still awaiting recovery/valid feedback.
- `generation_probe`: one in sixteen private-synthesis pairs, recorder only.
- `generation_capture_pending`, `generation_guides_pending`,
  `generation_consumer_pending`: dependencies pending on the CPU near enqueue
  for those sampled pairs; counts can overlap.
- `generation_enqueue_to_start_ms`: sampled CPU ExecuteCommandLists to first
  existing GPU timestamp, using that queue's adjacent GPU/QPC calibration.
- `generation_enqueue_to_end_ms`: same enqueue to final generation timestamp.
- `generation_queue_inputs_ready_ms` / `generation_queue_inputs_pending_ms`:
  split queue delay by sampled receipt state. Missing receipts are not called
  ready. A pending dependency may finish immediately afterward, so this does
  not individually time its wait or prove it caused all measured delay.
- `generation_calibration_cpu_ms`: calibration call overhead.
- `generation_calibration_failed` / `generation_calibration_invalid`: API or
  invalid correlation, excluded rather than reported as zero delay.

No additional GPU command lists, query heaps or timestamps are allocated.
Reuse the existing four timestamps and completed readback ring. Calibration
is sparse and adjacent to enqueue, not a mapping reused across idle periods.
Correlated records are consumed on slot reuse, so the last few pairs can remain
unreported at shutdown or reconfiguration. Generation completion does not
include display copy/compositor execution or measure motion-to-photon latency.
GPU timestamps are bottom-of-pipe markers, not exact first-shader timestamps.
Input waits and GPU contention are included in queue delay, not separated by it.

## Validation and headset test

Ten Windows tests pass, plus four Python analyzer fixtures. Scheduler tests
cover 72/90/120 Hz, sustained pending frames, two-real-frame recovery, synthetic
and invalid readings, unknown fallback, resets and preserved absolute cadence.
The actual Windows pacer exercises the fence probe with recording disabled.

The production NVIDIA/FFX GPU fixture deliberately blocks a capture receipt
for 60 ms after generator initialization. Correlated measurement reports
70.316 ms to GPU start and 73.001 ms to generation end, confirming that the
previous GPU-work timer omitted this wait. Stereo output, leases, queue joins,
shared bypass and cross-queue depth tests also pass with D3D12 validation.
This artificial delay is test-only and is not a game performance result.

Headset benefit remains unverified until a new run. Use the last resolution,
native NVIDIA framegen and adaptive pacing, external OFXR off. Hold RT and RR
fixed; natural play with NR off, then NR on, then off. Compare settled compositor
delivery, source age, phase steps and queue delay. Do not accept increased
submission count alone as success.

## Reference

[Microsoft D3D12 timing](https://learn.microsoft.com/en-us/windows/win32/direct3d12/timing)
documents queue-specific timestamps, GPU/QPC calibration, bottom-of-pipe
semantics and idle clock drift. No stable-power-state override is used.
Existing V557-derived schedule/queue code retains its LGPL attribution.
