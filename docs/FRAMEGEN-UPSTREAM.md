# Native framegen delivery and optional V557 reconstruction

This change addresses two separate symptoms in one integrated framegen package: irregular SteamVR delivery under load, and doubled/ghosted moving edges with the existing OFA + FidelityFX reconstruction. It keeps the port's native Presenter and OpenXR session; it does not embed an external OFXR API layer or replace the game launcher.

## Controls and runtime behavior

- F10 > FRAMEGEN > **SteamVR framegen optimizations (restart game)** controls presentation/synthesis queue isolation, physical refresh correction, producer gating, submission pacing and advance preparation together. Defaults on. Preference changes are saved immediately; the active session changes only after restarting the game, because the XR graphics-binding queue is fixed at session creation. The UI distinguishes requested and active states.
- Steam Link uses SteamVR and is eligible. VDXR and other non-SteamVR runtimes bypass this scheduling package. Existing external-OFXR queue protection remains in place.
- Upstream **Adaptive real-frame pacing** remains a separate live control; it is not replaced or relabeled as this work.
- **NVIDIA OFA + V557 (experimental)** is an additional selectable backend. Existing backend values/defaults remain intact. V557 is available with VDXR as well as SteamVR. It uses current-camera pre-warp, flow cost confidence and unidirectional midpoint composition. It does not interpolate from game motion/depth guides. Generated image pose/FOV matches its current-camera reconstruction, while content time is the midpoint.
- Flight recording is opt-in and off by default. It distinguishes accepted real/generated/repeated submissions, SteamVR compositor reports, source age and GPU timings. Accepted submissions are not proof of unique displayed headset frames; missing compositor/calibration data stays missing. The delayed capture tool supports saved-input reconstruction comparisons.

## Efficiency and ownership

The complete candidate includes both-eye preparation before prior-display waits, removal of unused real-output copies, and pinned immutable native captures in place of a duplicate V557 history ring. Capture owners survive until exact GPU receipts complete. Backend changes, recentering and teardown retain the same output/consumer ownership safeguards. The optional borrowed-history route reduces explicitly counted allocations by about 222 MiB at 3144x3000 per eye, compared with the initial copied-history adapter; this is not a claim about total driver VRAM.

Private presentation requires HUD/overlay copies to use the runtime queue and their exact producer fences too. Those changes are part of the same package, not unrelated overlay redesign. Bypassing the package restores the application queue route.

## Evidence and limits

The runtime implementation shipped in fork RC1 was tested in natural gameplay on PSVR2/SteamVR, Quest 3/Steam Link and Quest 3/VDXR. The user confirmed that the V557 option corrected the bike-windscreen ghosting on PSVR2 and VDXR. Saved-input comparisons matched the archived V557 reconstruction pixel-for-pixel. The runs were not matched-scene benchmarks and do not establish a universal FPS gain. The upstream-adapted PR binaries are build/test artifacts, not a claim of a new headset test on stock 0.2.3.

V557 remains optical-flow based and depth-unaware for translation; other effects may look worse. It requires NVIDIA OFA for this path, with the existing FidelityFX fallback on unsupported hardware. Longer capture leases can briefly reduce free capture slots. DLSS-G retains its separate generator route. This PR deliberately contains no scope RR isolation, haptics, controller mappings, body changes or installer changes; the scope fix can merge independently.

## Build, tests and source

Normal CMake builds compile `externals/v557-reconstruction` using Windows SDK fxc and the port's pinned FidelityFX/NVIDIA SDKs. All V557 source adaptations and shaders are provided in this repository. Licenses/provenance are included in distribution output. Original port files retain their existing license; the reconstruction subset is LGPL-3.0. Rebuild the main target to relink modifications; no additional runtime DLL, registry registration or tray tool is required.

For Windows regressions, run `tools/flight_recorder_tests/build.ps1` with the production JSON/OpenXR/FidelityFX directories. Build `tools/framegen_replay` with `OPENXR_INCLUDE` and `FFX_SDK`, then run `framegen_replay.exe --v557-test` for production-adapter GPU ownership/backend-switch/borrowed-history tests. `tools/frame_capture_tests` and `tools/hud_submit_tests` cover delayed capture and runtime-queue HUD ownership. Run `python -m unittest discover -s tools/flight_recorder -p "test_*.py"` for telemetry parsing.

Validation uses the compatible CP2.5 RED4ext SDK 5246e659 and cached pinned dependencies, as the released test build did. This PR does not change upstream's SDK submodule pin. Related design details: [package latch](fixes/steamvr-package-toggle.md), [V557 adapter](fixes/v557-reconstruction.md), [flight recorder](fixes/framegen-flight-recorder.md).

On the upstream extraction, all 16 Windows flight/pacing/queue regressions passed, the V557 production-adapter GPU suite passed on shared/private queues, the delayed-capture and HUD submission tests passed, and all four Python telemetry-parser tests passed.

Full MSVC x64 Release build passed on the upstream 0.2.3 extraction. Validation DLL SHA-256: `f6abd356021dba1dcacfa3530d7cb577d2306a6cfe7ecde80bc9bc259c62f7d5`. No game files were deployed by this validation.
