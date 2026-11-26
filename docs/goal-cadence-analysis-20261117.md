# Application cadence: traffic and motorcycle, PID26944

These are saved OpenXR Simulator traces from a real street scene. The moving-bike
runs started at the same measured position and travelled approximately9.1m;
traffic phase and world simulation were not identical between FG modes. They are
not a V557/PR203 backend comparison and do not measure physical headset scanout.

The C++ analyzer was extended without changing production pacing or the existing
fixtures/geometry/optics modes. Original `fg-*-delivery.json` and raw inputs remain
unchanged. New results are adjacent `fg-*-cadence-analysis.json` files in
`traffic-26944`, `bike-moving-off-26944`, `bike-moving-on-26944`, and the supporting
`bike-stationary-26944` directory under `scratch/goal-audit-20261117`.

## Clock and measurement distinctions

Trace row0 is QPC milliseconds sampled after xrEndFrame; row1 is predicted XrTime
nanoseconds; row7 is the reported period; rows9–11 are wait/end/select durations.
Predicted spacing uses only successive XrTime differences. Completion spacing
uses only successive QPC differences. No absolute XrTime−QPC subtraction or
unconverted display-deadline comparison was performed.

Two important missing-data cases were found in the source:

- `waitMs` is populated only when `CyberpunkVR_XrRateLog` is enabled. All saved
  rows contain zero, but the old harness did not record that gate. These are
  **unavailable/ambiguous wait timings**, not evidence of zero-cost xrWaitFrame.
- ResubmitLastFrame rows (`kind=-2`) encode literal zero for end/select because
  that path does not measure them. New statistics exclude those placeholders;
  the counts below make the exclusion explicit. Other repeated frames can have
  real end/select measurements and remain included.

## Primary four traces

All report a nominal11.111111ms period. There were no non-increasing predicted
times, backwards real source serials, or dropped trace rows. The before/after
snapshots recorded zero new empty-frame submissions in all six scenarios.

| Scene / FG | Accepted cycles | Real / generated / repeated IDs | Repeated share | Accepted-cycle rate Hz |
|---|---:|---:|---:|---:|
| Traffic off |1107|764 /0 /343|31.0%|89.72|
| Traffic on |1107|547 /545 /15|1.36%|89.82|
| Moving bike off |390|279 /0 /111|28.5%|89.27|
| Moving bike on |387|191 /189 /7|1.81%|89.32|

“Real” means a newly selected real source identity in the application trace, not
a pixel-uniqueness proof. Autonomous compositor reprojection/repetition between
application submissions is outside these counts.

| Scene / FG | Post-End interval p50 /p95 /max ms | Predicted spacing p50 /p95 /max ms |
|---|---:|---:|
| Traffic off |11.12 /18.85 /70.54|11.1111 /12.11 /60.32|
| Traffic on |11.01 /18.42 /53.61|11.1111 /12.77 /47.60|
| Moving bike off |11.18 /18.58 /58.95|11.1111 /11.90 /59.25|
| Moving bike on |11.14 /19.06 /61.72|11.1111 /12.18 /54.50|

The18–19ms completion p95 is substantially larger than predicted-spacing p95.
Short/long completion intervals also balance closely: traffic off90/90, traffic
on80/81, moving off33/30, moving on38/31 (short<0.5nominal periods,
long>1.5periods). That supports a pattern of completion-time bursts around the
nominal cadence. It does not establish visible judder. Rare47–60ms prediction
gaps remain in these windows; the timeline is not perfectly uniform.

| Scene / FG | Measured end samples /unmeasured resubmits | xrEndFrame p95 /max ms | FG Select p95 /max ms |
|---|---:|---:|---:|
| Traffic off |766 /341|7.43 /59.41|0.0077 /0.2283|
| Traffic on |1107 /0|6.93 /45.49|2.45 /15.80|
| Moving bike off |286 /104|8.65 /54.50|0.0066 /0.1684|
| Moving bike on |387 /0|7.14 /53.62|3.12 /17.14|

Most end calls are short (median0.20–0.25ms); there is a6–12ms shoulder and rare
long calls. The measured current end-call duration correlates with completion
spacing (Pearson r=0.75/0.67/0.79/0.70 in table order). FG-select correlation is
much weaker with FG off (0.06/0.02) and moderate with FG on (0.31/0.38).
Previous end duration correlates with the next predicted-spacing duration
(0.68/0.48/0.80/0.74), consistent with an expensive end call delaying subsequent
application/runtime scheduling. These correlations include outliers and are not
proof of a single cause. The same-cycle end/cycle correlation also shares a term
by construction (the end call is part of that cycle), so it is not an independent
causal test. The lagged result describes ordering, not a GPU/driver attribution.

A desktop144Hz period is6.94ms, near part of the end-call shoulder, but the
distribution is not sufficient to assign that shoulder to desktop present/DWM.
Simulator compositing, video capture, overlays and GPU/driver waits can all be
inside the measured call. A144Hz desktop and90Hz reported runtime are distinct
clocks. Do not patch port pacing based on this numerical resemblance.

## Content sequence

FG off has frequent real/repeat sequences rather than a new image identity every
runtime slot. Unique-real predicted intervals have medians11.93ms (traffic) and
11.28ms (moving bike), but p95 approximately22.3–22.4ms. This is a concrete
application-side mechanism for uneven motion at a90Hz target when real content
arrives less often. It is compatible with a monitor/headset difference, but the
trace alone does not prove the user's headset symptom.

FG on is predominantly alternating `RGRG`. Traffic has545 generated→real
transitions, no generated→generated transition, and only one real→real transition.
Moving bike has188 generated→real transitions, no generated→generated transition,
and two real→real transitions; the last generated row has no successor in the
window. Longest accepted-repeat runs are one traffic slot and two moving-bike
slots. Real and generated categories both have approximately22.22ms median
predicted spacing, consistent with interleaving at the11.11ms runtime period.

Thus these traces do **not** show a persistent half-rate submission stream with
FG enabled. They show mostly regular content interleaving plus rare disruptions.
A windshield artifact can still be an interpolation/occlusion/pose-quality issue;
accepted cadence alone cannot evaluate V557 reconstruction or validate its
claimed improvement. Identical saved temporal inputs with sufficient history and
both-eye image review are still required for that comparison.

## Supporting stationary runs and future timing

Stationary bike off:753accepted,523real/230repeated (30.5%), completion p95
19.79ms, prediction p9512.57ms, end p959.06ms/max85.30ms. Stationary bike on:
752accepted,369real/367generated/16repeated (2.13%), completion p9517.36ms,
prediction p9512.45ms, end p955.97ms/max49.93ms. The same broad pattern appears
without vehicle travel; it is not confined to moving-bike geometry.

Future traffic/bike harness windows now preserve `CyberpunkVR_XrRateLog`, enable
it only for the timed window with a short settling interval, record its endpoints,
and restore the exact previous value afterward and on cleanup failure. Source
inspection confirmed ReportXrFrameRates logs one batch per second, not per frame;
its in-memory counters still add diagnostic work. Next SteamVR null traces can
therefore separate measured wait and end costs. Null remains a desktop runtime,
so those results must also be kept separate from headset scanout evidence.

Validation: rebuilt C++ analyzer1/1 PASS, including independent clock offsets,
completion bursts with uniform prediction, category cadence and exclusion of
unmeasured resubmit timings. Python cleanup3/3 PASS includes exact rate-log
restoration despite fixture cleanup failure. No production pacing change made.
