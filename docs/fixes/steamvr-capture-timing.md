# Matched capture and synthesis timing, November 7, 2026

## Why this candidate measures rather than retunes

Consumer overlap was active but yielded75.46 vs73.73 compositor FPS estimates
and16.18 vs15.40ms generation enqueue-to-end in differing natural scenes. Start
delay remained~7.9ms. Pending capture/guide/consumer counts overlap and cannot
attribute that delay. User approved investigating it while keeping RT+NR ON.

Source audit found color capture already signals immediately after its existing
command-list submission. Inputs::Submitted signals copied guides on their own
submitted lists. AcquireInputs requires the exact same producer queue and frame/
pose/origin; OpenXRCapture selects these already submitted inputs under image
submission lock before enqueuing color capture. Thus pending guides are not proof
of an additional independent7ms wait: matching guide work precedes color capture
on the same queue. No fence can safely be removed to infer a performance fix.

## Measurement

With flight recorder ON, native FG enabled, and SteamVR targeted, one capture
serial in16 records two GPU timestamps in the EXISTING capture command list:
start before capture operations, end after color/depth/sharpen recording. Resolve
the two query values on that same list. Calibrate producer GPU clock to QPC just
before ExecuteCommandLists. Store the successful capture fence/value and share
this sample with the exact published frame, then the generator's work slot.

The sampled generation uses its existing four timestamps and separate synthesis
queue calibration. Both queues convert to common CPU QPC milliseconds; raw GPU
ticks are never subtracted across queues. No timestamp from an adjacent or merely
latest frame is paired. Maps only occur after capture and generation receipts
complete. Results arrive when a generator work slot is reused, like existing
metrics; one-second windows are approximate.

New timing fields, all from matched pairs:

- capture_enqueue_to_start_ms: game-queue backlog/scheduling before capture begins.
- capture_gpu_span_ms: elapsed GPU span of capture list, not game render duration.
- capture_enqueue_to_end_ms: total from capture enqueue to its end timestamp.
- generation_before_capture_ready_ms: portion of generation's start delay before
  the matching capture end timestamp (zero if capture had already ended).
- generation_after_capture_ready_ms: remainder after that boundary, including
  synthesis-queue backlog, interqueue wake/scheduling and the query resolve/fence
  publication following the capture end timestamp. This is NOT all shader cost.

The 'ready' boundary is the final capture GPU timestamp, immediately before query
resolve and fence signal; it is not a separate measured fence-signal timestamp.
These observations never control synchronization. Cross-calibration inconsistency
larger than0.25ms is rejected; a smaller negative boundary gap is clamped. The two
generation components sum to the same matched generation start delay. Compare
paired samples, not unrelated means with different sample counts.

Events: capture_probe_queued, capture_probe_skipped, capture_probe_paired,
capture_probe_invalid. Queued probes can be menus, skipped captures or frames not
chosen for generation, so queued need not equal paired. Existing1-in16 generation
calibration remains; a capture sample additionally requests matching calibration.

## Lifetime and overhead

Four reusable query/readback slots per manager/device; no image buffers or extra
GPU submissions. Allocation only on initial recorder sampling or device change,
not every frame. Slots cannot be reused while a frame/work reference exists or
GPU receipt is incomplete. Aborted/failed submissions quarantine slots instead
of unsafe reuse/unbounded allocation. Existing command-resource leases pin query
heap/readback across aborted recordings and private capture-list lifetime.
Readbacks are nonblocking, no CPU/GPU waits added. Sampling and calibration have
finite overhead; this is a diagnostic candidate, not a zero-cost release change.
VDXR, disabled recorder and disabled native FG do not record capture queries.

Rendering, queue priorities, pacing recovery, consumer-overlap policy, scope fixes,
SDK and user settings remain unchanged. RR investigation remains deferred.

## Validation

Production NVIDIA/FFX fixture deliberately delays the game capture queue60ms,
records real capture queries and passes their exact sample through Generator.
Asserts capture and generation pre-ready delay exceed15ms, paired sample appears,
no invalid cross-clock sample, and pending readback is not mapped. Existing
stereo midpoint/output pixels, leases and blocked previous consumer tests pass.
Math cases cover ready-before-enqueue, pending capture, invalid order, calibration
tolerance. Pool test proves bounded capacity and no reuse of pinned references.
D3D12 error/corruption messages fail the GPU suite. A second production case
finishes capture first, then blocks synthesis60ms; it must attribute >15ms to
post-capture delay. Both matched samples must validate, preventing a probe that
labels every delay as input production.

See capture-timing-provenance.json, capture-timing-deployment.json and archived
logs for tested build/hash and actual deployment. Test RT+NR ON throughout,
RR OFF, SteamVR native NVIDIA FG/adaptive ON; natural play, then exit normally.
The purpose is to choose whether producer backlog, capture work or post-capture
synthesis scheduling is the next change. No claimed FPS benefit from this build.
