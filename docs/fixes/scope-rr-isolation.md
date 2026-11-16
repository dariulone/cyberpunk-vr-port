# Scope SR/RR state isolation — 0.2.2 test port

The native camera scope reused MAIN DLSS/RR camera, viewport and size cache.
A save load with an equipped scope alternated tiny and full-size RR creation,
causing severe lag. Scope isolation uses the native recursive DLSS lock, a private
viewport (43565301), independent camera/cache and consistent frame token. The
ViewportHandle ID is at offset4A0; offset498 is its struct version, not the ID.
MAIN state is restored after each call. Scope completions/tags are excluded from
headset framegen capture and per-eye SR processing.

Native scope input/output are341x341 at1:1. MAIN Ultra Performance inherited by
RR requested114x114 input instead. For the private scope only, matched1:1 options
use DLAA mode; all camera matrices, exposure, presets and MAIN settings remain.
The original scope rendering graph is preserved. The rejected graph fallback
is not included. Upstream 0.2.2's existing per-eye RR path remains enabled and unchanged for headset views.

Prior fork validation (not a 0.2.2 headset result): production state/ABI tests cover500 alternating frames, resets,
owner replacement, frame gaps and unwind restoration. RR options tests cover
1:1, true upscale, MAIN/VRCAM, future/off options and preservation of other fields.
MSVC Release build passed. Latest run created MAIN2096x1000->6288x3000 and
scope341x341->341x341 only, successful actual private-viewport evaluation, no
recreation storm and clean shutdown. User reports scope/RR appears fixed.
Installed DLL SHA256 A2B044EBE7A112F936F66FF2A12DC8B436DF7F63B4F81DF238866979FC6398E9.

A separate19:07 crash involved native garment-component cleanup through CET;
this change does not claim to resolve it. Upstream0.2.1 wardrobe/body fixes remain
separate work. Trace diagnostics are bounded startup notices, not per-frame logs.

Tests (from repository root):
```
g++ -std=c++20 -Iinclude -Iexternals/streamline/include tools/scope_dlss_state_test.cpp -o /tmp/scope-state
/tmp/scope-state
g++ -std=c++20 -Iinclude -Iexternals/streamline/include tools/scope_rr_options_test.cpp -o /tmp/scope-options
/tmp/scope-options
```

## 0.2.2 adaptation, November 6

Base: upstream tag `0.2.2`, commit `0a2f4f10`; ported from fork commit
`2af8db99`. This is a separate checkout, not a replacement of the maintained fork.

0.2.2 adds its own per-eye RR handling. Exclude the scope from BOTH the SR and
RR options caches, as well as tags/constants, per-eye dispatch and headset FG
completion capture. Share the official Streamline RR header instead of keeping
a second partial definition of the options structure in EngineStereoDlss.cpp.
The scope still evaluates RR through the native graph on its private viewport.
No V557 backend or frame pacing change is included.

The reported VDXR crash at 08:48:55 was a GPU invalid-call/device-removal failure
with acceleration-structure work in progress. It is not proven to share the
same immediate cause as the earlier repeated scope/MAIN context recreation.
This patch addresses the confirmed missing isolation; runtime success must be
checked separately.

Build uses the known CP2.5 SDK `5246e659d783e5834cc08a90e56c8932c9ed3449`
through the CMake SDK override; the stale upstream SDK submodule is not used.
Do not regenerate SdkLayouts.gen.inc. Build-only CMake support reuses the exact
0.2.2 release scc_lib.dll and pinned prebuilt FidelityFX libraries. MinHook source
and headers match upstream's submodule SHA, and the new IK submodule remains at
its 0.2.2 pin. `build-scope.ps1` records all build inputs.

Validation: state/actual SDK ABI and RR options tests pass with GCC warnings as
errors (vendor includes treated as system headers) and ASan/UBSan with leak
checking disabled. Installed game bytes match the guarded RR options entry and
native recursive DLSS lock/EnterCriticalSection import. Release build and
headset results are recorded in the deployment receipt after completion.

Test: use the same scoped weapon; enable RT/RR, aim, holster/re-equip, then load
another save. Confirm no stall/crash/noisy scope and that logs show actual private
viewport `43565301`, successful RR evaluation and 1:1 scope dimensions. New scope
messages are bounded to a handful per session; no per-frame logging was added.
