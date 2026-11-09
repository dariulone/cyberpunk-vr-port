# SteamVR synthesis preparation during presentation idle time

Candidate: prepare-ahead-v1, 2026-11-07. Headset performance remains unverified.

## Evidence

The display-deadline run (flight-run-20261107-165355) found 33/47 sampled
synthetic frames completing after the estimated runtime boundary, against
0/45 real frames in the same RT+NR window. CPU handoff was timely on average.
Generation waited about 11.75 ms to begin, mostly for the captured endpoint.
The prior implementation only queued synthesis when selecting that synthetic
frame for immediate presentation. Accepted submissions are not displayed FPS.

## Change

During an existing real/repeat pacing hold, inspect the latest complete stereo
capture and queue the next midpoint if the already-created NVIDIA backend is
compatible. The same XR thread owns all Presenter/Generator state. No new
worker or wait interval is introduced. At most two attempts are made, only with
at least 2 ms remaining before the existing handoff deadline; a new-capture
event can wake the second attempt. Drain a stale event before the first latest-
state check so it cannot consume the second attempt.

Do not insert the future synthesis output fence into the current display queue.
Join it when the next selection actually consumes the prepared output. Preserve
the exact midpoint then corresponding real-endpoint order even if newer captures
arrive. Keep one prepared pair, using existing capture pins rather than new
image allocations. GPU completion retains input/color leases. Toggle, recenter,
projection, quality/backend and gameplay changes cancel stale prepared state.

Eligibility is SteamVR + native NVIDIA OFA + adaptive pacing + private queues.
Preparation uses try-lock on capture metadata, releases it before GPU submission,
and excludes SDK initialization/reconfiguration and busy command allocators.
Synthetic presentation never runs this preparation callback because it reads the
output that future synthesis would overwrite. VDXR and other backends retain
the existing path. The improved producer gate and scope/RR fixes are unchanged.

## Validation and limits

14 flight/GPU tests plus the production HUD queue test pass; MSVC Release builds.
Production Presenter tests cover ordering, cancellation and pinned endpoints.
A D3D12 test deliberately blocks future synthesis while confirming current real
queue completion; the generated output is joined only afterward and its source
lease remains live while the GPU reads are outstanding. Idle-wait and production
pacer tests cover stale events, original deadline, attempt limit, real/repeat
eligibility, synthetic exclusion and callback-overrun measurement.

This advances submission opportunity, not a guarantee of GPU completion. High-
priority synthesis may still compete with game/compositor GPU work. SDK calls
can overrun despite preflight checks, so CPU time and deadline overruns are
recorded. Next normal RT+NR run must show useful prepare_ahead usage/lead time,
fewer late synthetic completions, and no regression in real delivery or image
age. Frame counts alone do not establish a gain. RR memory retention from the
last test is separate; this candidate does not change Cheeky, RR or settings.
