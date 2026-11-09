# Completed-capture pacing credit — November 7, 2026

**REJECTED after11:56 runtime test.** Restored producer-gate-v1 binary and source; see flight-run-20261107-115604/analysis.md and producer-credit-rollback.json.

## Purpose

Producer-gate-v1 improved user-reported smoothness and natural-run compositor
estimate72.05->81.71, source age34.13->28.87ms, capture queue22.14->16.57ms.
It reduced accepted real throughput43.24->35.69/s: CPU cadence and5.44ms added
GPU hold ran consecutively. User authorized recovering throughput without losing
the improvement. Keep79b478b4 producer-gate-v1 as the tested baseline.

## Conservative coordination

Do not simply overlap away the entire gate: the extra hold is what coincided with
less backlog. Coordinate successive releases instead. A capture completing within
its bounded GPU wait earns HALF its measured GPU hold against the NEXT CPU pacing
interval, capped at half the GPU budget (2.78ms at90Hz,2.08ms at120Hz,3.47ms at72Hz;
maximum4ms). The credit cannot exceed the CPU hold still remaining. It is consumed
once, never accumulated. Successful GPU completion is required; already-ready,
timeout, wait-failed, missing or device-lost results earn zero. Completion after
the requested budget (OS overscheduling) earns none. No anticipated FPS claim.

Credit is discarded on missing capture, invalid period, refresh or adaptive
interval change, disabled FG/pacing, inactive backend, or long pause. It is owned
by the existing producer pacing mutex, not a separate asynchronous controller.
Credited/GPU-held frames anchor their next schedule to actual release; no absolute
deadline debt/catch-up bursts. Sustained timeouts restore v1 release timing after
any previously earned one-shot credit is spent. The GPU gate itself, maximum
wait, capture publication order and exact receipt tracking are unchanged.

Only SteamVR native NVIDIA FG/adaptive pacing is affected. VDXR and other backends
retain existing deadlines. No settings, resolution, NR behavior, scope fixes,
queue priorities, resource fences or interpolation algorithms change. No new
GPU work/allocation, timer, callback or hot-path log text is introduced.

## Validation and recording

Producer gate fixture retains real D3D12 exact-fence/blocked-queue checks. Added
production policy tests cover one-shot credit, completed/overscheduled waits,
timeout/failure/ready/missing results, missing current capture, refresh/adaptive
change, menu pause, disabled/reset, already-late producer, remaining-hold clamp,
and100-frame72/90/120Hz simulations including30 consecutive pending captures.
All11 flight/GPU tests plus production HUD pass; MSVC Release builds successfully.
Flight recorder marker: upstream022-steamvr-producer-credit-v1.
producer_credit_ms records planned CPU-hold reduction (zeros included), not a
claim of measured GPU acceleration. Existing GPU hold/results/capture attribution
remain available. OS wait overshoot can change actual time saved.

## Headset acceptance

Keep RT+NR ON, RR OFF, native NVIDIA FG, adaptive pacing ON, SteamVR. Natural play
and normal exit. Compare with archived11:34 producer-gate-v1 run. Desired: recover
real/FG throughput while retaining lower source age/capture backlog/readiness and
compositor delivery. A higher accepted rate alone does not pass; repeats and
compositor estimate must not be called unique-content FPS. Initial candidate
keeps half the completed hold deliberately; a larger credit needs evidence.
No claim of a guaranteed improvement until the headset run is assessed. RR crash
investigation stays deferred. Rollback DLL is preserved by deployment script.
