---
name: settings-plumbing
description: The repetitive ~9-place pattern to add a live-controls setting in CyberpunkVRPort
metadata: 
  node_type: memory
  type: reference
  originSessionId: 0353ad85-8636-45bd-8b7d-635741e848f9
---

Adding one live-controls setting (e.g. `xr_motion_predict_ms`) touches ~9 spots. Miss one
and it silently won't parse/persist/apply. Checklist:

In `src/dxgi_proxy.cpp`:
1. `struct LiveControls` — add `volatile <type> xrFoo;`.
2. `EnsureLiveControlFileExists()` — add a default `fprintf(file, "xr_foo=...\n");`.
3. `PollLiveControls()` — (a) local default `<type> xrFoo = ...;`, (b) `sscanf_s` parse
   (both `xr_foo=%..` and `xr_foo = %..` variants), (c) add to the `changed` ||-chain,
   (d) apply to `g_liveControls.xrFoo = ...;`, (e) append to the big `Log(...)` printf.
4. `MakeLiveControlsUiState()` — `state.xrFoo = g_liveControls.xrFoo;`.
5. `PersistLiveControlsUiState()` — `fprintf(file, "xr_foo=...\n", state.xrFoo);`.
6. `SetLiveControlsUiState()` — `g_liveControls.xrFoo = state->xrFoo ...;`.
7. Add getter `extern "C" <type> GetFoo() { return g_liveControls.xrFoo; }`.

In `src/live_controls_ui.h`: add `<type> xrFoo;` to `struct LiveControlsUiState`.
In `src/imgui_overlay.cpp`: add a slider/checkbox in the relevant `CollapsingHeader`.
In any consumer (e.g. `openxr_manager.cpp`): `extern "C" <type> GetFoo();` then call it.

Settings flow: external/CET UI or the ImGui overlay writes `vrport.ini` (game dir);
`PollLiveControls` reloads it on file-mtime change. See [[project-cyberpunkvrport]].
