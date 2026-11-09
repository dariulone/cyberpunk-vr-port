# SteamVR overlay presentation queue, November 6, 2026

Candidate overlay-queue-v1 builds on readiness-v1; RR crash work is deferred.
The RR dump, instructions, debugger outputs and allocation/consumer analysis
remain in flight-run-20261106-181956 and the repository OKF.

## Problem and change

Projection/depth copies already run on the SteamVR session binding queue.
HUD, interaction, vehicle, surveillance and hit-marker layers still copied on
the game queue. Each update transferred runtime image availability to the game
queue, then signaled its current tail back to the runtime before releasing the
image. Newer RT/NR rendering can intervene. This also delays later captures via
the game queue's reacquire wait on earlier presentation work.

In the prior 18:10 SteamVR run, 235–270s has 1,583 pairs of overlay joins over
35.209s (44.96/s); 290–303s has 776 over13.084s (59.31/s). This establishes the
path was active, not how much of its GPU delay was caused by unrelated work.

HUD canvas rendering remains on the original game queue. Every published canvas
now carries the successful producer fence/value. The XR copy runs on the runtime
binding queue and waits only on that exact receipt. The existing three copy
slots retain their source Frame until each slot's consumer fence completes;
producer reuse also requires sole ownership. Missing receipts fail closed on
the private path, with no game-tail fallback. Failed producer Signal does not
publish a new canvas. Source format, resolution, masks and styling are unchanged.

F10 copies likewise use their existing published canvas receipt and retain its
lease. The CPU-generated FPS overlay uploads directly on the runtime queue.
All three use native XR wait/release calls on the binding queue; they need no
application/runtime round trip. VDXR and external-OFXR shared binding still use
the original queue, with no additional source waits. SteamVR gate applies to
all headsets using that runtime, not only PSVR2. No scheduling constants,
framegen algorithm/quality, RT/RR/NR settings or scope code are changed.

## Validation

MSVC Release build passes. Ten recorder/queue/scheduler tests pass, including
actual GPU producer delay, exact source receipt, a blocked next game frame,
pixel verification, source pin lifetime and shared-queue bypass. Separate test
links the production HudQuad.cpp and PresentationQueue.cpp: three fenced slots,
retained sources, repeat metadata, XR timeouts, resize, disable, private-queue
copy while the game tail stays blocked, missing receipt rejection. D3D12 error
checks pass. OpenXR calls are mocked in these fixtures; headset benefit remains
unverified. No claim that this alone fixes all RT/NR delivery loss.

## Recorder and test

Header upstream022-steamvr-overlay-queue-v1. New overlay_source_ready/pending
counts sample source fences near GPU enqueue; they are not display readiness.
The older presentation_application_wait/runtime_wait counters should stop
incrementing for port-owned overlays. Compare compositor estimate, source age,
whole-presentation readiness and synthesis queue delay against readiness-v1.

Use SteamVR/PSVR2 with native NVIDIA framegen, adaptive pacing and recorder ON,
external OFXR OFF. Keep RT ON, RR OFF (separate RR transition crash), use NR OFF
then ON during natural walking/riding/combat. Check HUD and F10 remain usable.
No need to revisit VRCAM or change rendering quality for this comparison.

Only the port DLL is deployed, with a verified backup and unchanged INI and
UserSettings hashes. Source/DLL hashes: overlay-provenance.json.
