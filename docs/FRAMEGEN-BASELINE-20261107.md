# Tested 0.2.2 framegen baseline — 2026-11-07

This branch preserves the locally tested SteamVR work on upstream tag 0.2.2
(`0a2f4f10`). It is a checkpoint, not a public release or a claim of perfect
90 FPS delivery. The installed port DLL is unchanged by these commits:
`36a4cbb7943e9347194cd3a109d326c3a2673bee3a0e465e96f71982b23e4854`.

**Update:** the separate [SteamVR package toggle](fixes/steamvr-package-toggle.md)
was added after this checkpoint. The snapshot below describes the original
November 7 baseline; use the toggle document for current controls.

## Runtime scope and controls

| Change | Applicability |
|---|---|
| Physical refresh correction | Positively identified SteamVR; valid, fresh compositor reading required |
| Submission grid / readiness feedback | SteamVR, threaded projected submission, native FG and Adaptive Pacing enabled; unavailable readings fall back |
| Private synthesis queue | SteamVR native NVIDIA OFA/FidelityFX and pure FidelityFX; native DLSS-G keeps its original generation queue |
| Private presentation queue | SteamVR session, except detected external XRFrameBridge layer; fallback on unavailable private queue |
| Bounded producer GPU gate | SteamVR native NVIDIA FG with adaptive pacing |
| Prepare future synthesis during existing idle hold | SteamVR native NVIDIA FG with adaptive pacing and private presentation queue |
| Flight recorder, scope/RR fixes, shared queue-ownership plumbing | Shared code; recorder is opt-in and scope fixes have their own guards |

VDXR does not opt into SteamVR refresh correction, pacing or private queues.
Steam Link uses SteamVR and IS included; there is no separate Steam Link exclusion.
This is runtime detection, not a PSVR2 headset whitelist. Other SteamVR headsets
can use physical refresh readings too. The latest optimization sequence was
validated on PSVR2/SteamVR; equal benefit for Steam Link is not established.

Adaptive Pacing is a useful existing live pacing switch, but is NOT a complete
stock-versus-patched routing toggle: the session's presentation queue and the
backend's private synthesis route are not disabled by that checkbox. A future
advanced **SteamVR framegen optimizations** opt-out is worthwhile for compatibility
and A/B testing. It should gate all the SteamVR adaptations coherently, default
on for the validated baseline, and require session recreation/game restart since
OpenXR's D3D12 queue binding is selected at session creation. Do not force the
SteamVR-specific implementation onto VDXR, which lacks its compositor feedback.
No new toggle is introduced by this checkpoint; installed behavior is preserved.

## Included behavior and limits

The port now separates production, synthesis and runtime presentation ownership
with explicit cross-queue fences; overlays/HUD retain matching ownership. Physical
refresh replaces throttled application cadence only with credible readings.
Bounded producer gating and GPU-aware presentation avoid growing source backlog.
Preparing future synthesis spends an existing idle interval, without changing
resource lifetime or making current presentation wait on that future output.

Rejected HUD-ready and completed-capture-credit experiments are not in active
code. Their documents are historical and marked rejected. Prepare-ahead is kept
as the installed baseline, but its independent FPS benefit was not demonstrated.
Source-frontier checkpoints and the flight recorder remain available for diagnosis.
Their optional overhead is not a rendering optimization; recording is off by
factory default, although the local test INI still enables it. No logs, captures,
DLLs, dumps, settings or local deployment receipts are committed.

The strongest measured improvement earlier in the series was producer gating:
natural-play compositor estimate72.05 ->81.71, sourceage34.13 ->28.87ms, capture
queue22.14 ->16.57ms, accompanied by user-reported smoother play. Scenes were not
deterministically matched, so these are observed runs, not a controlled benchmark.
Accepted real rate43.24 ->35.69/s demonstrates why submissions alone must not be
sold as experienced FPS. Compositor estimates also include repetition and do not
identify real/generated scanout content.

Latest RT+NR, RR-off run: compositor estimate77.39FPS, real35.33/s and generated
35.30/s accepted, sourceage28.51ms. Prior run estimate82.97,36.07/s each,28.89ms.
Settings identical; natural gameplay differs. No improvement claimed from the
last diagnostic. Capture waits14.89ms; ~90.4% precedes both eye upscale checkpoints.
Capture itself0.142ms; capture-ready to synthesis-start0.449ms; generation4.64ms.
Cheeky's existing sampled timers measure SR1.594ms,NR2.541ms,containingpipeline
4.147ms per sampled evaluation. They are not eye-paired or additive stereo totals.
Source GPU work is now the stronger lead than further capture-placement changes.

Known outstanding issues: occasional bounded producer wait failures/prepare
callback overruns; RR-on transition crashes in some NR/runtime combinations;
NR/RR memory retention and whole-system VRAM use are not resolved by pacing.
Do not claim NR's entire measured cost is neural computation: it includes guide
preparation, encode, temporal filtering and decode/composite. Latest NR tracked
textures stayed~389MiB/twofeatures and released at exit; not total VRAM accounting.

## Build and validation

The RED4ext.SDK revision used to build this binary is
`5246e659d783e5834cc08a90e56c8932c9ed3449` (patch2.5 types), with no semantic local
SDK edits. The gitlink is pinned to this revision for fresh checkouts. FBT IK is
`39f65bef1f46b0ea945dbd46aafbe4c7aaf89b8d`. FidelityFX remains upstream's pinned
`c6efa6bf7f2027b3ec94f28578bb5965eabb9e55`. Do not replace SDK layouts manually.
Optional CVR_SCC_LIB_PREBUILT and CVR_FRAMEGEN_PREBUILT allow reuse of the pinned
release compiler and matching SDK libraries; defaults retain source builds.
CVR_RED4EXT_SDK_DIR can point to a separate checkout of the pinned SDK.

Production MSVC Release and15 Windows framegen/GPU checks plus the production
HUD test passed for the installed candidate. Tests include delayed/abandoned/
replayed GPU checkpoints, queue fences, exact capture receipts, fallback routes,
pacing clocks, bounded slot reuse and recorder validity. Rechecked at commit time.
Python analyzer tests cover exported metrics and avoid submission-FPS fallbacks.

Run `tools/flight_recorder_tests/build.ps1` with explicit `-JsonInclude`,
`-OpenXRInclude` and `-FidelityFxSdk` dependency directories from the production
build (`-CMake`/`-CTest` can give full executable paths). SDK path ends in `sdk`.
HUD test CMake accepts `-DHUD_OPENXR_INCLUDE=<headers>`; run ctest Release afterward.
Python: `python -m unittest discover -s tools/flight_recorder -p 'test_*.py'`.
The separately deployed Cheeky timing-only change is retained as a reproducible
source patch under tools/cheeky_stage_timing, not silently mixed into port code.

See [presentation tracking and attribution](STEAMVR-DELIVERY.md) and the individual
[producer gate](fixes/steamvr-producer-gate.md),
[synthesis queue](fixes/steamvr-synthesis-queue.md),
[presentation queue](fixes/steamvr-presentation-queue.md),
[source checkpoints](fixes/steamvr-source-frontier.md) documents.
OFXR-derived files retain LGPL notices and Valve OpenVR retains its BSD license;
see licenses/. No upstream PR, push or binary release is implied by these commits.
