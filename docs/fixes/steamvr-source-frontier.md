# Frame-matched producer GPU checkpoints

Diagnostic candidate source-frontier-v1, November7. No pacing, queue priority,
image processing, resource ownership or user setting policy changes.

The source capture's GPU execution costs~0.14ms while waiting~15–16ms to start.
Earlier synthesis submission and Reflex+Boost did not solve source readiness.
Measure native frame/pose/origin-matched depth copy, motion copy and each eye's
upscaler-return/output-copyback, followed by capture enqueue/start/end and
synthesis enqueue/start. The upscaler call includes intercepted work recorded
before it returns, including NR on that route. It is NOT proof of final game
postprocessing completion; the following gap can contain necessary postprocess,
HUD or other game work. Do not assume an earlier color checkpoint is safe to
capture directly or drop any current dependency.

One native frame in16, six marker stages (2eyes x3), each two adjacent GPU
queries in existing game command lists. No extra GPU command lists/waits.
Completion signals on the owning submission queue guard readbacks. CPU submission
observation/calibration occurs after Execute; fast GPU markers may correctly
precede it. CPU recording timestamps and raw calibrated endpoints retained.
Capture retains its existing sparse schedule plus sampled source frames where
its observed image label matches, then revalidates identity at actual submission.
Frame0/pose wrap, queue and origin changes cannot silently mix identities.

Bounded8-frame registry,32-record logger handoff. GPU resources pinned through
CommandResources; query storage is reused only after every recorded list resets,
submitted uses complete, and no captured/generator reference remains. Replay,
duplicate stages, mismatched identities and unfinished/invalid timestamps are
rejected. Aborted recordings/device failures never manufacture completion.
No pointer to a game image is retained by this diagnostic. Pool exhaustion skips
rather than growing or waiting. Toggling recorder off stops new markers; lifetime
tracking finishes existing markers. The pool/device references are bounded.

Tests:15 flight/GPU checks plus production HUD pass; production source-timeline
test intentionally blocks GPU queue then delays capture, verifies separate delays,
GPU-safe pending reads, exact identities, replay/duplicate/abandon rejection and
24+completed reset/reuse cycles. Recorder test checks raw JSON identities and
bounded overflow accounting. Python analyzer verifies queue partition and invalid
samples. Release builds. Source hooks in Inputs.cpp/EngineStereoDlss.cpp are
strictly diagnostic additions to existing scope-corrected files, not rollback.

Output source_probes includes complete mask63, rr state, per-eye pose, nativeframe,
origin and all QPC timestamps. Missing markers are explicit partial masks; analyzer
requires63. tools/flight_recorder/source_timeline.py separates RR states and reports
capture queue interval before latest checkpoint versus after. Signed relative
timings preserve calibration uncertainty; not actual scanout or photonlatency.
SourceMarkerRecorded/Skipped/Duplicate/Replay/SignalFailed and SourcePaired/Invalid/
RecordDropped counters identify missing evidence. Compare paired observations,
not unrelated timing means. Timestamp queries/fences/calibration have nonzero
measurement overhead; no performance improvement claimed for this build.

Next run RT+NRon,RROFF,Reflex+Boost unchanged,nativeNVIDIAFG/adaptiveON. Normal
movement/gameplay,exit normally. If checkpoints finish near capture start, investigate
producerGPU rendering/backlog; if substantially earlier, identify intervening work
before moving a capture boundary. Restore baseline from backup if diagnostic
cost materially changes behavior. Headset evidence pending.
