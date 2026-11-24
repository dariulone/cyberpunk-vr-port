# Isolate native scope DLSS/RR state

A camera scope renders a second image with different dimensions from the main stereo view. Sharing the native DLSS camera/cache/viewport lets those evaluations replace one another's RR state. With a scope equipped, enabling RR or loading a save can repeatedly switch tiny/full-sized contexts, producing severe stalls and unstable scope output.

The fix gives `vr_optics_feed` its own viewport 43565301 and independent camera/cache, uses the engine's recursive DLSS lock and a consistent frame token, and restores MAIN state after each evaluation. It corrects the CP2.5 viewport-ID address to 0x4A0; 0x498 is the Streamline structure version. Scope calls are excluded from MAIN per-eye SR/RR caches and headset framegen input/completion capture. The scope retains its native rendering graph, including RR.

For matched 1:1 scope input/output, the private RR options use DLAA instead of inheriting MAIN Ultra Performance. A separate guarded scope-only scaler correction restores full target resolution when RR is active. `xr_scope_rr_full_resolution=0` and restart restores the previous scope sizing without disabling state isolation. Default 1 can cost more GPU time: 1024 square is about 9 times the pixels of 341 square, for the scope image only. MAIN quality and stereo rendering settings are unchanged.

The full-resolution hook verifies the CP2.5 scaler bytes and required native interfaces; unknown code/layouts do not opt into that path. Temporary flag changes preserve unrelated bits and restore on nested/exception unwinds.

## Validation

The production helper tests cover 500 alternating MAIN/scope frames, resets, frame gaps, owner replacement, unwind restoration, actual Streamline struct layout, RR 1:1 versus real upscale, unchanged matrices/presets/MAIN/VRCAM, scope-only full-resolution eligibility and flag restoration:

```sh
g++ -std=c++20 -Wall -Wextra -Werror -Iinclude -isystem externals/streamline/include tools/scope_dlss_state_test.cpp -o scope-state
./scope-state
g++ -std=c++20 -Wall -Wextra -Werror -Iinclude -isystem externals/streamline/include tools/scope_rr_options_test.cpp -o scope-options
./scope-options
g++ -std=c++20 -Wall -Wextra -Werror -Iinclude tools/scope_resolution_test.cpp -o scope-resolution
./scope-resolution
```

These tests pass on the upstream 0.2.3 extraction. The implementation was exercised in the maintained fork, where the user reported that the RR/scope lockup stopped. The freshly adapted upstream binary has not received another headset test. This does not claim to fix every RT/RR/NR GPU crash or eliminate all temporal scope softness.

The PR is independent of the framegen package. Its only changes in framegen input hooks exclude private scope viewports; it imports no scheduling, V557, haptics, controller, or SDK changes. Local full-plugin validation reuses compatible CP2.5 SDK 5246e659 and cached dependencies through build-only CMake overrides; those overrides are not shipped as scope changes.

The same three production-helper tests also pass under AddressSanitizer and UndefinedBehaviorSanitizer (leak detection disabled).

Full MSVC x64 Release build passed on the upstream 0.2.3 extraction. Validation DLL SHA-256: `e75d62891d5a9ab5860fd82652e3e59cdda78de291a45668b34240d15bf7f783`. No game files were deployed by this validation.
