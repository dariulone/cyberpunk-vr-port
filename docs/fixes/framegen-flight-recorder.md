# Frame-generation flight recorder (0.2.2 test build)

F10 > FRAMEGEN > **Flight recorder** starts/stops recording live. The checkbox
persists as `xr_framegen_flight_recorder=0/1`. Default is off; the local diagnostic
deployment enables it. It works with the FPS overlay and verbose diagnostics off,
with FG enabled or disabled. **Mark performance problem** adds a marker count.
The status below the checkbox reports actual writing, waiting, or a file error.

Logs: `red4ext/plugins/cyberpunkvrport/log/framegen-flight-*.jsonl`.
One file per recording interval, timestamp and PID in its name. A background
worker flushes once per second and on normal stop. Each file stops at16MiB;
toggle off/on to open another. No automatic deletion of older recordings.
An abrupt crash may leave a partial final line; the analyzer ignores that line.

## What it records

- Actual runtime/headset, requested/effective generator, quality/flow scale,
  enabled/pacing/cap settings, actual input/output dimensions and display period.
- Captured frames, each eye's input count, missing-neighbor feedback, rejected
  interpolation pairs, waiting-for-input cycles, history priming and generator
  queue-slot failures. Reasons overlap: do not sum them into an overall lost count.
- GPU generation command-list duration and full span including OFA/queue gaps,
  using the existing asynchronous timestamp/readback machinery, enabled only
  while recording or the original overlay needs it. No new GPU waits or images.
- Actual real-frame pacing interval, successful XR real/generated/repeated
  submissions, fallback resubmissions, empty ends, allocator pressure, capture
  stalls, source-capture age, xrWaitFrame/xrEndFrame duration and XR errors.
- SteamVR's V557 compositor estimate plus settled records presented zero times,
  repeat/mispresentation/drop reports, history holes, reprojection flags and
  compositor-reported app/compositor GPU times. Reader gates on the current PID,
  waits for90 accepted submissions and3seconds before attaching, and uses a
  background OpenVR connection, never a Scene application.

Generated-queued is not completed or displayed. Accepted means successful
xrEndFrame, not proof of physical presentation. SteamVR FPS is an estimate,
not exact unique-image FPS or motion-to-photon latency. Real/generated attribution
inside compositor records is unavailable. VDXR gets no substitute estimate.
The compositor fields overlap and must not be added into a total loss number.
Atomic aggregate boundaries are approximate. Transitions are separated in the
analyzer; timestamped settings are the end-of-window state. Timings are mean/max,
not fabricated percentiles. A one-second log cannot reconstruct every deadline.

## Analysis and tests

`python tools/flight_recorder/analyze.py <log>` prints per-mode rates and loss
signals; `--json` emits aggregates. Test natural walking/riding/fighting with the
same selected generator. Keep the recorder on while switching FG off/on if
comparing. No stationary scene or repeated VRCAM switches are required.

Windows fixtures under `tools/flight_recorder_tests` exercise actual recorder
worker/file output, overlay-off counters, concurrent writers, null VDXR delivery,
flush, restart and file errors, plus the reused SteamVR estimator fixtures.
Python analyzer tests distinguish accepted submissions from compositor delivery,
retain missing data and separate transitions. Headset validation remains pending.

No generator, scheduling, graphics setting or scope algorithm is changed by this
patch. Recording has CPU/file-I/O and GPU-timestamp overhead; default remains off.
Reader provenance/license: `docs/STEAMVR-DELIVERY.md`, `licenses/OFXR-LGPL-3.0.txt`
and `licenses/OpenVR-LICENSE`. The settled-frame reader was amended to populate
GPU-time/reprojection fields that the prior 0.2.0 diagnostic reader left empty.
