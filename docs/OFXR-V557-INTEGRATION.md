# Private OFXR V557 integration

The FRAMEGEN menu's former **NVIDIA OFA + FidelityFX** choice now selects
**OFXR V557 (NVIDIA)**. This is the patched Djules V557 capture/synthesis/presenter
path, loaded directly by the plugin. It is not merely a replacement optical-flow
shader. Other native generators remain available, but switching to/from V557
requires OpenXR runtime recreation or a game restart; they cannot run together.

The package contains `ofxr-v557/CyberpunkVR_OFXR.dll` and its private OpenXR loader
beside `CyberpunkVR_Stereo.dll`. A private explicit API-layer manifest lets the OpenXR loader build the chain. There is no tray, registry
registration, or system-wide installation. Keep the whole private folder when
installing. If an external OFXR layer is already loaded, the private backend is
bypassed and F10 tells the user to disarm the external tray and restart the game.
The integration does not alter external registrations itself.

## Settings and logs

F10 > FRAMEGEN:

- Frame generator: **OFXR V557 (NVIDIA)**.
- NVIDIA quality: **Slow**, optical-flow resolution: **50%** for the tested profile.
- Enable frame generation toggles V557 live. Quality changes also apply live.
- **V557 flight recorder** persists as `xr_framegen_logging` in `vrport.ini`.
- The recorder status reports whether the backend actually has a file open. A
  requested checkbox alone is not proof that recording succeeded. Setting changes
  are followed by V557 on subsequent XR frames; allow roughly half a second.

Logs: `red4ext/plugins/CyberpunkVR_Stereo/ofxr-v557/ofxr-bridge-flight-*.log`.
They include process/build/runtime receipts, session/swapchain configuration,
API errors, capture/synthesis timing, and SteamVR delivery estimates. The existing
V557 recorder limits each file to 256 MiB and re-emits session metadata after
wrapping or enabling recording mid-session. There is no GPU fingerprint readback.
Recording still has ordinary CPU/file-I/O cost. The mod's FPS panel measures mod
submissions; it must not be interpreted as V557 generated headset delivery.

Fixed initial compatibility configuration: 2X/shallow, separate panel generation
off, independent synthesis queues off, bidirectional off, Hybrid off, OFXR overlay
off, end-frame capture on. HUD/dialog/loot pixels retain application cadence;
V557 still handles their presentation/pose composition. The mod retains F10/F11.

## Source and build

Target: Cyberpunk VR Port 0.1.9.
Backend base: Djules `f64d4b0` (0.2.14.2 / V557), with the existing local
`f096694` panel/performance patch and `6746076` optional queue patch. The private
backend adds only `OFXR_IsFlightRecordingV1`, a read-only active-recorder receipt;
its rendering and scheduling code is unchanged from that patched V557 source.

CMake fetches the complete backend from
[satyaloka93/OFXR-Bridge](https://github.com/satyaloka93/OFXR-Bridge/tree/e750741e8ea3d46dde5c57de57523df2c1ea88bf),
pinned to `e750741e8ea3d46dde5c57de57523df2c1ea88bf`. For offline builds, set
`CVR_OFXR_SOURCE` to a checkout of that revision. Supply the backend SDK paths
(`XRFG_OPENXR_SDK_ROOT`, `XRFG_NVIDIA_OPTICAL_FLOW_SDK_ROOT`,
`XRFG_FIDELITYFX_SDK_ROOT`, `XRFG_DLSS_SDK_INCLUDE_DIR`,
`XRFG_VULKAN_INCLUDE_DIR`, and `XRFG_OPENVR_INCLUDE_DIR`) as described by its
source build instructions. These are build dependencies, not an external OFXR
installation requirement for players. `cmake/EmbeddedOfxr.cmake` builds the backend and
copies it and the OpenXR loader into the plugin output. The optional
`CVR_FRAMEGEN_PREBUILT=ON` reuses already-built libraries from the pinned
`CVR_FIDELITYFX_ROOT` when the SDK's shader build dependencies are unavailable.
Do not substitute V577/V681 or an unpatched V557 source directory.

Tests in `tools/ofxr_bridge_tests` exercise the real adapter and V557 DLL against
the backend's fake runtime: core and extension dispatch, instance recreation,
quality mapping, saved logging setting, active recorder/file receipt, generator
exclusion, and external-layer detection. Backend call-chain tests cover split-eye,
cropped split-eye, HUD-panel bypass and SteamVR presentation. These are integration
checks, not an in-game performance measurement. The October 31 corrected game run averaged 85.1 estimated delivered FPS versus
84.7 for external V557 during active-generation samples in seconds 110-190.
Mispresented-frame rates were 4.3% versus 5.0%. Cheeky and scene conditions changed
between runs, so this supports recovery without establishing controlled parity.

OFXR is LGPL-3.0; the backend remains a separate replaceable library. Retain its
license and source/patch provenance with any distributed build. The mod retains
its own existing license.

## Loader-managed integration correction (2026-10-31)

The original adapter manually wrapped an already-created loader instance. That
placed private V557 above implicit observer layers, so its generated frame calls
also traversed those observers. It also ignored forwarded instance-create changes.
The revised adapter requests V557 as a private explicit layer through the normal
loader. Implicit observers remain above V557 and do not see its internal frames.
The private manifest search path is added only around instance creation, retains
existing paths, and is restored afterward. No registry or Cheeky changes are made.
Installed external OFXR manifests conservatively bypass the private backend.

Tests now use the real loader and backend with a fake headset runtime. An upper
observer verifies that V557's own runtime query does not traverse it while the
application query does. Tests also verify search-path restoration, instance
recreation, recording controls and exclusion. This corrects the integration
boundary; the in-game observations above are separate from those structural tests.

### Adapter tests (Windows, Visual Studio 2022)

After building the plugin, configure the standalone test project with your actual
source, SDK and output paths (no game or headset is needed):

```powershell
cmake -S tools/ofxr_bridge_tests -B build-ofxr-tests -G "Visual Studio 17 2022" -A x64 `
  -DCVR_OFXR_SOURCE=C:/src/ofxr-v557 `
  -DXRFG_OPENXR_SDK_ROOT=C:/sdk/OpenXR-SDK `
  -DXRFG_VULKAN_INCLUDE_DIR=C:/sdk/Vulkan-Headers/include `
  -DCVR_OFXR_DLL=C:/build/plugin/ofxr-v557/CyberpunkVR_OFXR.dll `
  -DCVR_OPENXR_LOADER_DLL=C:/build/plugin/ofxr-v557/openxr_loader.dll
cmake --build build-ofxr-tests --config Release
ctest --test-dir build-ofxr-tests -C Release --output-on-failure
```
