# One complete SteamVR framegen optimization toggle

F10 > FRAMEGEN > **SteamVR framegen optimizations (restart game)**.
This one setting controls the complete package: presentation queue isolation,
synthesis queue isolation, physical refresh correction, producer gating,
submission pacing and advance preparation. Defaults ON.

Changing it saves the next-launch preference immediately. Restart the GAME to
apply; turning framegen off/on only changes generation and does not apply the
package preference. F10 shows active state for the current session and a restart
notice when the saved preference differs. Upstream Adaptive real-frame pacing
remains its own live control. Dynamic backend/model switching remains available.

## Why the single startup boundary

Presentation isolation is essential to the tested package. OpenXR binds its
D3D12 presentation queue at session creation. Applying only timing/synthesis
live leaves a partially optimized route and is a misleading complete ON/OFF
comparison. Therefore the complete package is latched once at startup, used by
both runtime graphics binding and all timing/synthesis consumers.

The temporary split/dynamic UI from d25642ae is retired at the user's request.
`xr_framegen_steamvr_optimizations` remains authoritative. The obsolete
`xr_framegen_steamvr_presentation_isolation` key is accepted and ignored, is no
longer written, and cannot override the complete package in either key order.
Existing unrelated settings and all 0.2.3 selected backports remain intact.

VDXR/non-SteamVR runtimes bypass the package; Steam Link uses SteamVR and is
eligible. Existing external OFXR protection can prevent private presentation;
DLSS-G retains its own generator routing. General rendering/scope fixes and
independently chosen diagnostics are not switched off by this control.

## Validation

Production MSVC Release succeeded; all 16 Windows framegen tests passed. Actual settings/latch fixture
covers persistence, independent adaptive control, legacy split-key acceptance
without override, runtime bypass, no live queue switch and startup application.
Log/flight build marker: upstream022-steamvr-package-unified-v3. Recorder keeps
requested preference distinct from active package state. Headset validation of
this unified UI is pending; underlying package matches the pre-split build.

## Existing Quest / Steam Link comparison

Nov8 first run PID12108 had the full package ON; second run PID30280 started
fully OFF. Stable gameplay, menus/transitions/teardown excluded, priming stalls
retained. Same 90Hz, eye output 2856x3088 / input 952x1029, OFA+FidelityFX medium,
50% flow and upstream adaptive pacing enabled. Different natural gameplay:
this is supporting evidence, not a matched-scene causal throughput measurement.

| Metric | Full ON (first run) | Full OFF (second run start) |
|---|---:|---:|
| Selected gameplay seconds |118.72|129.78|
| Real accepted/s |34.79|36.35|
| Synthetic accepted/s |34.62|32.32|
| Fresh accepted sum/s |69.41|68.67|
| SteamVR compositor estimate |85.85|80.96|
| Source age mean ms |24.56|23.40|
| Pair gaps/s |0.18|4.00|
| Pacing budget max ms |24.04|66.67|

Main benefit is consistent pairing and avoiding prolonged pacing backoff, not a
large average fresh-frame throughput gain. Neither compositor estimate nor fresh
submission rate proves distinct decoded headset FPS; source age is not end-to-end
latency. Second run's later timing-only ON phase is separate: presentation remained
shared, so it did not test the complete package. Archives and weighted selection:
`integration-notes/quest-steamlink-20261108-1754/full-package-comparison.json` and
`full-package-assessment.md` (local diagnostic artifacts, not shipped).
