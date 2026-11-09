# SteamVR presentation tracking

Reader ported from local OFXR V557 branch cp2077/embedded-v557 (4150114eb918f8fdc8f087323561c1170560f904), src/layer/steamvr_delivery.cpp and include/xrfg/steamvr_delivery.hpp. Retained under LGPL-3.0 (licenses/OFXR-LGPL-3.0.txt); other original port files retain their existing licenses. OpenVR header retains Valve BSD license (licenses/OpenVR-LICENSE). Source and modifications must accompany distribution under the applicable LGPL terms; no public binary release is made here.

Uses the same V557 rolling-window estimator, now with a cumulative-statistics PID check before reading timing records. Worker polls at100ms and emits5-second aggregates even when the overlay is off. Attach waits for90 submissions and3seconds, borrows an existing context first, otherwise uses Background (never Scene). Stop reader before destroying session; release owned connection at instance destruction, never a borrowed context.

STEAMVR EST is an estimate from compositor timing/mispresentation history, not a photon measurement or exact unique-content FPS. No fallback to submission FPS. The log separately records presented and zero-present records, drop/mispresentation reports, extra presents and maximum client interval. These fields overlap and MUST NOT be added into a total lost count. Per-frame real/generated attribution is unavailable, so no presented-real or presented-FG number is claimed. App repeat submissions remain separately reported by framegen-summary; a compositor-presented record may itself contain repeated app content. Estimates are comparable to V557's estimator, not proof of equal smoothness.

## Native SteamVR pacing adaptation

The November6 handoff candidate adds SubmitSchedule.hpp, SteamVrPacer.cpp and
submit_margin.hpp, adapted from V557 e48d6fcf628943413401e5bb68f53244aef9947a
(src/layer/openxr_layer.cpp, include/xrfg/submit_margin.hpp), retaining LGPL-3.0.
It is a partial native presenter adaptation, not full V557 integration. Details
and validation limits: [handoff pacing](fixes/steamvr-handoff-pacing.md).

## Private synthesis queue adaptation

The November 6 synthesis candidate adapts the V557 HIGH-priority synthesis queue
and explicit input/output synchronization from the same e48d6fcf revision.
`SynthesisQueue.hpp/.cpp` retain LGPL-3.0 attribution. It applies to native NVIDIA
OFA/FidelityFX and pure FidelityFX on SteamVR; it does not replace the entire
OFXR layer or change VDXR/native DLSS-G. See [queue synchronization and tests](fixes/steamvr-synthesis-queue.md).

## Presentation queue completion

The subsequent November6 candidate adds Runtimes/PresentationQueue.hpp/.cpp,
retaining the same V557 LGPL attribution. SteamVR now receives a private binding
queue; projection/depth copies and native synthesis output route to it. Existing
application-rendered HUD/overlay images use explicit bidirectional ownership
fences. VDXR and external OFXR keep their binding. See
[presentation queue](fixes/steamvr-presentation-queue.md) for ownership, tests and
remaining headset validation.

## Current checkpoint

The authoritative consolidated scope, results and known limitations are in
[the November 7 baseline](FRAMEGEN-BASELINE-20261107.md). Earlier per-candidate
documents retain their historical test status. Steam Link follows SteamVR here;
VDXR bypasses SteamVR-specific adaptation. Adaptive Pacing is not a whole-queue
opt-out. No new opt-out control was introduced by the checkpoint commit.

A separate [SteamVR package toggle](fixes/steamvr-package-toggle.md) now provides
the coherent restart-required opt-out described by that checkpoint.
