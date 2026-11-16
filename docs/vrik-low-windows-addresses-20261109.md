# Stationary Quest 2 controller-driven hands: low Windows allocations (2026-11-09)

## Evidence

The supplied `temp/` diagnostics reproduce stationary avatar hands with working Touch buttons and sticks.
The reporter reproduced the issue with port versions **0.2.2 and 0.2.3**, using both **VDXR and SteamVR**
with Quest 2 Touch controllers. This version/runtime coverage is reporter-provided; the detailed logs
examined here identify VirtualDesktopXR.
The first run's `vrik_diag.txt` contains changing controller coordinates, but no resolved skeleton:
`headIdx=-1`, `boneCount=0`, `lastBoneBuf=0`, `matchCalls=0`, including the final capture with `bind=4`.
The second, verbose run reports both hands active with valid located poses in multiple samples, while
`poseApply` and `freshSolve` remain zero. Later samples also contain inactive controllers; the fix below
does not address runtime focus or tracking loss.

`animation_system_lookup.txt` reports `playerEntity=0x5978fb7f0`, `runtimeScene=0x1a0057e98`, no registered
animation-system game system, and `scannedAnimationSystem=0`. The observed pointer values are below
`0x0000010000000000`, the minimum address accepted as `HEAP` on Windows by `ClassifyQword`.
The known scene chain rejects a parent/system below that bound, and arming separately rejects a low
metaRig. Those three pointers were not recorded, so the exact rejected hop is not established by the
supplied dumps. The reporter confirmed normal controller-driven avatar hands after installing the test patch.

## Relationship to the existing Wine/Proton fix

`src/Natives/AnimGraph.cpp` already documents the same stationary-hands symptom under Wine/Proton
(issue #14, diagnosed by wundervrc). Its existing `IsRunningUnderWine` check selects a lower heap-address
bound, `0x0000000100000000`, while retaining `0x0000010000000000` on native Windows. The present report
shows that a Windows execution can also contain valid engine objects below the Windows bound.

This patch does not change the Wine detection, either bound, or the classifier used by untyped scans.
Instead, known scene-chain pointers, the cached system, and the player's metaRig use readable-memory
checks; the system's RTTI check still establishes its expected type. The prior Wine/Proton fix remains
in place. This is not a claim of a new Wine/Proton runtime test.

## Changes

- `src/Natives/AnimLookup.cpp`: validate readable memory for the known runtime scene and parent chain;
  validate the candidate's readable object header and retain the `worldAnimationSystem` RTTI check.
  Cached-system validation uses the same guard. No minimum heap address is assumed in this typed path.
- `src/Natives/VrikArming.cpp`: validate the known metaRig's complete header with `VRIK_IsReadable`
  instead of applying the heap-address classifier. Bone counts and binding checks remain in place.
- Untyped fallback scans and `ClassifyQword` remain unchanged. This avoids admitting additional arbitrary
  pointers into a scan that calls virtual RTTI methods.
- `tools/vrik_tests/low_address_lookup_test.py`: extract the production lookup and readable-memory
  functions, compile them with MSVC, and exercise real Windows allocations and protection flags.
  RTTI is a mocked boundary; the test does not launch the game or exercise the complete arming routine.

## Validation and remaining work

Run on Windows with MSVC x64 build tools:

```powershell
python tools/vrik_tests/low_address_lookup_test.py
```

Cases cover valid chains below 4 GiB, at 8 GiB, and at 2 TiB; wrong RTTI; null/invalid pointers;
an inaccessible scene/candidate; and a guarded parent page. The smoke test passed locally.
As a sensitivity check, the same generated harness was compiled with upstream's original lookup
functions and its original `ClassifyQword`/Wine detection: it rejected the valid low-address chain
(`FAIL: valid chain`), while the patched implementation passed.

The fork's separate test-release build completed a full Release compilation, and the reporter confirmed
that the hands now follow the Quest 2 controllers in game. That test build also used an updated SDK
and build-only setup adjustments, so the game test did not isolate this patch from the SDK change.
SDK, submodule, build-configuration, and CI changes are deliberately excluded from this upstream patch.
The isolated lookup test does not depend on the SDK and exercises the pointer-validation change itself.

Post-fix bone indices/counters were not supplied; save reload and equipment-change coverage was not
reported. Those remain useful follow-up checks because they rebind the skeleton.
