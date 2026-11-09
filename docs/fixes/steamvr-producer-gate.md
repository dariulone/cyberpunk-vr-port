# SteamVR bounded producer completion gate, November 7, 2026

## Evidence and objective

Matched258GPU samples showed22.11ms capture queue delay but0.14ms capture work;
7.46ms of8.49ms matched synthesis-start delay awaited capture. Capture completion
was followed by~1.03ms until synthesis started. Prior consumer-wait overlap did
not materially change this. These data identify upstream delay, not how much is
avoidable rendering cost versus render-ahead/contention.

This candidate tests bounded GPU-aware producer release. Success means capture
backlog/source latency falls without sacrificing useful compositor delivery.
If it simply reduces base FPS, frequently exhausts its budget without reducing
backlog, or is always already-ready, it has not solved the issue. Preserve the
prior375aceca diagnostic build and RT+NR settings for rollback/comparison.

## Change and threading

At game HookedPresent entry (after filtering port-owned mirror swapchains), clear
that worker's receipt. A successful capture Signal with both matching guides
publishes its fence/value into the worker's thread-local receipt. OnPresent,
XR notification/pump, desktop mirror and original DXGI Present run as before.
PaceRenderedFrame takes this exact receipt before its existing pacing mutex.
No newest-global-frame substitution; a failed/menu capture cannot reuse an old
receipt, and concurrently scheduled Present workers cannot overwrite each other.

After the existing CPU cadence hold, native NVIDIA FG with adaptive pacing active
on SteamVR checks completion. Already ready: no extra wait. Pending: poll between
high-resolution timer slices of at most0.5ms, using the existing pacing timer.
Requested additional budget is half physical refresh, capped at8ms (5.56ms at
90Hz,4.17ms at120Hz,6.94ms at72Hz). Deadline plus16-slice limit bounds operation;
OS scheduling may overrun a nominal timeout. Timer failure/device removal return
immediately. There is no blocking GPU fence registration, GPU queue wait, extra
submission or new image allocation. It never waits for interpolation/compositor.

Only the game Present worker is held; the pre-existing producer pacing mutex is
held as before. No XR worker, image submission lock or present/capture mutex is
held during this gate. Capture publication happens BEFORE waiting, so the XR
consumer can continue using it and previous frames. This explicitly preserves
the earlier publish-before-throttle fix. After a GPU-held release, seed the next
CPU cadence from actual release instead of catching up and refilling backlog.

Disabled native FG/adaptive pacing, absent valid cadence and not-ready-to-generate
branches return before the gate. VDXR and non-NVIDIA backends retain previous
cadence; gate uses runtime/physical refresh, not a PSVR2 device check. Turning off
existing adaptive pacing bypasses the gate; no new menu option/settings change.
Missing capture receipts bypass safely. Real-recovery, consumer-overlap and
matched capture diagnostics remain. RR crash investigation remains deferred.

## Validation and diagnostics

New producer_gate fixture checks missing/ready bypass, completion release, exact
budget, timer failure, stalled-clock iteration bound,72/90/120Hz budgets, per-worker
receipt isolation, one-time consumption and stale-receipt clearing. Real D3D12
queued fences verify that a completed capture passes even with later work blocked,
a blocked target times out, and completion resumes without stale registrations.
Production timer helper is used. D3D12 error/corruption messages fail the test.
Existing stereo output, leases, capture attribution, queue ownership and HUD
checks retained. Twelve tests total (11flight/GPU plus production HUD); build and
exact hashes in producer-gate-provenance.json, installation in deployment receipt.

Header upstream022-steamvr-producer-gate-v1. Counts producer_gpu_ready/wait/released/
timeout/wait_failed/device_lost/missing. producer_gpu_wait counts every attempted
pending wait; result counters separate success, timeout and failure. Timings
producer_cpu_hold_ms and producer_gpu_hold_ms separate the two stages; GPU hold
includes zero for initially-ready/missing cases, so inspect counts as well as mean.
No claim that waiting itself creates performance. Compare existing matched
capture queue/first-start/end timings, accepted real rate, actual compositor
estimate/zero-present records and source age. Accepted real+FG is not displayFPS.

Test natural play with RT+NR ON throughout, RR OFF, SteamVR/native NVIDIA FG and
adaptive pacing ON, then normal exit. No stationary scene/off-on sequence needed.
