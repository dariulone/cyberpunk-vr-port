# SteamVR handoff pacing candidate (November 6, 2026)

## Evidence and scope

The scanout-corrected PID20756 run still accepted about 33 real plus 33
generated frames per second, with 23 application repeats, while the compositor
estimate was 59 FPS in the 90–100 second window. 297/899 settled compositor
records were never presented. GPU generation was about 1.26ms. The adaptive
producer target stayed near 30ms after the slowdown; upstream's recovery from
31.6ms to 22.2ms requires about 85 healthy one-second windows plus its quiet delay.
These observations support addressing scheduling, but do not prove which
individual generated images were lost.

This candidate adapts selected presenter mechanisms from local OFXR V557
`cp2077/embedded-v557` at `e48d6fcf628943413401e5bb68f53244aef9947a`.
It does **not** install the full OFXR backend, producer/consumer release handshake,
or alter native interpolation/optical flow, XR display timestamps, poses, or scope
RR handling. Source attribution is retained in SubmitSchedule.hpp and
STEAMVR-DELIVERY.md; the derived components retain LGPL-3.0 terms.

## Behavior

* Positively identified SteamVR, native FG enabled, adaptive pacing enabled,
  threaded submit, renderable gameplay, and a fresh physical-refresh observation
  are required. VDXR never creates this pacer or opens its OpenVR reader.
* A bounded high-resolution timer holds inside the begun frame before selecting
  images. All real/generated/repeated submissions share one absolute scanout
  grid. No game capture mutex is held while waiting.
* At handoff, read SteamVR's remaining frame time. Only a fresh real endpoint
  steers phase; synthetic and repeated images are measured separately.
* Target margin follows the compositor's smoothed render start, with V557's
  1ms clearance and 1.5–3ms bounds. A whole-scanout error is never folded away
  modulo refresh. Quarter-error corrections, 250us deadband, bounded whole-error
  acquisition and eight-real-reading cooldown follow V557's measured controller.
* Advance the absolute grid by one period, then skip expired slots and deadlines
  within a quarter period after xrEndFrame returns. Do not add xrEndFrame's cost
  to every nominal period, and do not submit a catch-up burst after an overrun.
* Ordinary holds are bounded to one period, acquisition holds to two. Failed
  timers/readings/submissions, empty cycles, menus, FG disable and refresh changes
  clear scheduling state. Session shutdown destroys the reader after loop shutdown;
  existing instance-boundary connection ownership is unchanged.
* SteamVR-only adaptive recovery halves excess interval after two healthy seconds,
  with a 2%-of-base minimum recovery step. Existing 4% backoff and 2x upper bound
  remain. The previous slow recovery remains byte-for-byte on other runtimes.
  This recovery policy is a new native adaptation, not an OFXR algorithm.

## Verification and recording

Six Windows fixtures cover recorder/estimator/refresh monitoring, 72/90/120Hz
submission schedules, unwrapped error acquisition, cooldown, long-call recovery,
margin bounds, disabled and VDXR gates, reader failure, and session recreation.
Four analyzer fixtures cover existing log semantics. Simulated recovery reaches
the base within ten healthy seconds; this is not a measured gameplay result.

Flight header: `upstream022-steamvr-handoff-v1`. New counters include paced cycles,
unavailable pacer, phase movements and XR_FRAME_DISCARDED. New timings separate
real/generated/repeated handoff gaps, remaining time and xrEndFrame duration,
plus hold time, probe overhead, target margin and absolute phase correction.
Counts remain application-side. Compositor estimates and zero-present records
are still required to assess benefit; successful submissions are not delivered FPS.

Next run: keep the previous RT-on/RR-off/NR-off settings, NVIDIA FG,
adaptive pacing and flight recorder ON, and play naturally. No stationary or
VRCAM baseline is required. Judge compositor loss/delivery, recovery and feel;
the candidate is not validated in the headset until that test.

## V1 result and V2 revision

V1 gameplay PID23148 (100–180s):42.72real+42.12generated accepted/s,
3.26repeats/s,74.87compositor-estimated FPS,1008/7001records never presented.
Uneven motion remains. Real/synthetic handoff gaps10.41/12.32ms, mean
remaining3.28/2.08ms, generation span3.97ms. These aggregates do not prove GPU
readiness at each handoff.85pacer-unavailable events were not reason-separated.

V2 supersedes the preparation order above: Begin only arms the runtime gate;
BeforeSubmit holds AFTER native generation and copy are queued. The hold remains
inside a begun frame, before xrEndFrame, with no capture mutex held. This allows
GPU work to overlap the wait and takes preparation cost out of handoff spacing.
It may change source-frame age/selection phase; those remain recorded and must be
assessed. No synchronous GPU wait or new render resources are introduced.

Individual missing/nonpositive/out-of-range readings skip phase feedback; three
consecutive failures clear the schedule. Timer failure clears it immediately.
New reason counters distinguish these cases. Existing copy-fence completion is
sampled before the hold for generated images and at handoff for real/generated
images; ready/pending counts describe that fence, not compositor delivery.
CPU xrEndFrame timing starts AFTER the pacing hold. New prepare_before_hold_ms
records native preparation time. Header upstream022-steamvr-handoff-v2.

Six Windows fixtures pass, including preparation-before-hold ordering, variable
preparation cost on an even schedule, bounded bad-reading grace, runtime gates
and lifecycle. Four analyzer tests pass. Gameplay benefit remains pending.
