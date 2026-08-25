#include <windows.h>
#include "Stereo/VrcamConfig.hpp"   // cname_hash, for the device camera name
#include <psapi.h>
#include <xinput.h>
#include "Utils/SharedSlots.hpp"   // CyberpunkVR_Hands_Shared slot map (single source of truth)
#include <cstdint>
#include <cstddef>
#include <cstdio>
#include <cstdarg>
#include <cstring>
#include <cmath>
#include <share.h>
#include "Utils/AobScanner.hpp"
#include "Overlay/LiveControlsUi.hpp"
#include "Overlay/LauncherDialog.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include "Runtimes/RuntimeFovCorrection.hpp"
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Scripting/Natives/ScriptGameInstance.hpp>
#include <iostream>
#include <MinHook.h>


#include "Hooks/SwapChain.hpp"
#include "Utils/LogThrottle.hpp"
#include "Hooks/Trampoline.hpp"
#include "Utils/MemorySafe.hpp"
#include "Core/Telemetry.hpp"
#include "Core/LiveControls.hpp"
#include "Core/VrCoreShared.hpp"
#include "Core/CoreInternal.hpp"
#include "Camera/CameraLink.hpp"
#include "Hooks/Hook.hpp"
#include <string>

FILE* g_logFile = nullptr;
char g_gameDir[MAX_PATH] = {};
char g_liveControlPath[MAX_PATH] = {};
char g_launcherConfigPath[MAX_PATH] = {};
char g_backendModulePath[MAX_PATH] = {};
FILETIME g_lastLiveControlWrite = {};
// Bridge files in the CET VRIK mod folder (CET sandboxes a mod's relative paths to
// its own folder). dxgi WRITES vrik_settings.ini (mouse-Y flag, CET reads it); CET
// WRITES vrik_recenter.ini (a counter on save load) which dxgi polls to recenter.
char g_vrikSettingsPath[MAX_PATH] = {};
char g_vrikRecenterPath[MAX_PATH] = {};
FILETIME g_lastVrikRecenterWrite = {};
static const int kNoRecenterBaseline = -2000000000;
int g_lastVrikRecenterCounter = kNoRecenterBaseline;

uintptr_t g_gameModuleBase = 0;
size_t g_gameModuleSize = 0;
void Log(const char* fmt, ...);


static constexpr int kEnableNativeSetterTracers = 0;

int ClampRuntimeMode(int value) {
    return value == 1 ? 1 : 0;
}

LiveControls g_liveControls = {};

// Verbose per-frame logging (ClipCursor / depth-diag / hook spam). Off by default so
// the tester log stays readable; toggled live from the F10 Debug section. Not persisted.
volatile int g_verboseLog = 0;
int g_launcherWidth = 2048;
int g_launcherHeight = 2048;
int g_launcherHmdType = 0;
// DEBUG tick-box in the launcher, persisted as debug= in vrport-launcher.ini. It is the
// master switch for every probe, census and dump in the mod -- see ApplyLauncherDebugGate
// in debug_gate.cpp for why the gating happens once at startup rather than per read.
int g_launcherDebug = 0;

// Moved to src/Core/LauncherConfig.cpp: paths and the launcher ini.

// Publish the mouse-Y flag for the CET VRIK mod (it reads this from its own folder).
void WriteVrikSettingsFile() {
    InitRuntimePaths();
    int v = g_liveControls.xrDisableMouseY != 0 ? 1 : 0;
    FILE* file = _fsopen(g_vrikSettingsPath, "w", _SH_DENYNO);
    if (!file) { Log("VRIK bridge: FAILED to open %s for write\n", g_vrikSettingsPath); return; }
    fprintf(file, "disable_mouse_y=%d\n", v);
    fclose(file);
    static int s_lastLogged = -1;
    if (v != s_lastLogged) { s_lastLogged = v; Log("VRIK bridge: disable_mouse_y=%d -> %s\n", v, g_vrikSettingsPath); }
}

// Moved to src/Core/LiveControls.cpp: polling the live-control file and the overlay UI state.

extern "C" void PrepareStartupLiveControls() {
    static bool g_dialogShown = false;
    EnsureLiveControlFileExists();
    PollLiveControls();
    LoadLauncherConfig();

    if (!g_dialogShown) {
        g_dialogShown = true;
        ShowLauncherDialog();
    }
}

// Moved to src/Core/FirstLaunch.cpp: the game settings this port was tuned against.

extern "C" void SetWindowResolutionAndPersist(int width, int height) {
    SaveLauncherConfig(width, height);
}

extern "C" void SetHmdTypeAndPersist(int hmdType) {
    g_launcherHmdType = hmdType;
    // Persiste insieme a width/height già in memoria
    SaveLauncherConfig(g_launcherWidth, g_launcherHeight);
}

extern "C" void ApplyLauncherDebugGate();   // debug_gate.cpp

extern "C" int GetLauncherDebug() {
    return g_launcherDebug;
}
// Re-arms the gate on the spot. The plugin loads vrport-launcher.ini during RED4ext Main,
// which is long before the launcher dialog can be shown (that happens at swapchain
// creation), so the startup gate necessarily runs on the PREVIOUS session's value. Applying
// again here is what makes ticking the box take effect in the session you ticked it in,
// instead of the next one -- the same one-launch-behind trap the resolution pick had.

extern "C" void SetLauncherDebugAndPersist(int on) {
    g_launcherDebug = on != 0 ? 1 : 0;
    SaveLauncherConfig(g_launcherWidth, g_launcherHeight);
    ApplyLauncherDebugGate();
}

extern "C" int GetCurrentHmdType() {
    return g_launcherHmdType;
}

// Persist the VR runtime choice (0 = OpenXR default runtime, 1 = SteamVR/OpenVR)
// into vrport.ini. Applied on the next OpenXR init, which happens AFTER the
// launcher closes — so picking it here takes effect for this launch.
extern "C" void SetRuntimeModeAndPersist(int mode) {
    g_liveControls.xrRuntime = ClampRuntimeMode(mode);
    PersistLiveControlsUiState(MakeLiveControlsUiState());
}

extern "C" int GetCurrentWindowWidth() {
    return g_launcherWidth;
}

extern "C" int GetCurrentWindowHeight() {
    return g_launcherHeight;
}

UINT GetForcedRenderWidthValue() {
    uint32_t w = 0, h = 0;
    if (OpenXRManager::Get().GetRecommendedRenderTargetSize(&w, &h) && w > 0) {
        return w;
    }
    return 0;
}

UINT GetForcedRenderHeightValue() {
    uint32_t w = 0, h = 0;
    if (OpenXRManager::Get().GetRecommendedRenderTargetSize(&w, &h) && h > 0) {
        return h;
    }
    return 0;
}

static UINT GetForcedWindowWidthValue() {
    if (g_launcherWidth > 0) {
        return static_cast<UINT>(g_launcherWidth);
    }
    return GetForcedRenderWidthValue();
}

static UINT GetForcedWindowHeightValue() {
    if (g_launcherHeight > 0) {
        return static_cast<UINT>(g_launcherHeight);
    }
    return GetForcedRenderHeightValue();
}

// UNUSED since the DLSS resolution override went quiet -- that was its last caller, and the name
// only ever made sense while every preset was Pico-shaped. Left in place because it is the one
// helper that answers "is the launcher square?", which the AER-era code kept asking.
[[maybe_unused]] static UINT GetForcedSquareResolutionValue() {
    const UINT fw = GetForcedWindowWidthValue();
    const UINT fh = GetForcedWindowHeightValue();
    if (fw > 0 && fh > 0 && fw == fh) {
        return fw;
    }
    return fw > 0 ? fw : fh;
}

extern "C" UINT GetForcedSwapchainWidth() {
    return g_launcherWidth > 0 ? static_cast<UINT>(g_launcherWidth) : 0;
}

extern "C" UINT GetForcedSwapchainHeight() {
    return g_launcherHeight > 0 ? static_cast<UINT>(g_launcherHeight) : 0;
}

extern "C" UINT GetForcedDisplayModeWidth() {
    return GetForcedWindowWidthValue();
}

extern "C" UINT GetForcedDisplayModeHeight() {
    return GetForcedWindowHeightValue();
}

extern "C" UINT GetForcedWindowWidth() {
    return GetForcedWindowWidthValue();
}

extern "C" UINT GetForcedWindowHeight() {
    return GetForcedWindowHeightValue();
}

extern "C" int GetDisableRoll() {
    return 0;
}

extern "C" float CyberpunkVR_HeadsetDefaultFovDeg();

extern "C" float GetForcedFov() {
    // The user's number always wins. Only when vrport.ini leaves this at 0 does the per-headset
    // measured default apply -- see CyberpunkVR_HeadsetDefaultFovDeg in OpenXRManager.cpp for why the
    // default is routed through this value and not through the runtime's eye frusta.
    const float fromIni = g_liveControls.xrForceFov;
    if (fromIni > 1.0f && fromIni < 170.0f) return fromIni;
    return CyberpunkVR_HeadsetDefaultFovDeg();
}

extern "C" float GetMenuFov() {
    return g_liveControls.xrMenuFov;
}

extern "C" float GetMenuFollowDeg() {
    const float v = g_liveControls.xrMenuFollowDeg;
    return (v >= 5.0f && v <= 90.0f) ? v : 60.0f;
}

extern "C" int GetMenuRectMode() {
    return g_liveControls.xrMenuRect;
}

extern "C" int GetSyncSequential() {
    // alternate-eye pose-pair locking. On the SteamVR runtime, latch ONE head pose
    // per alternate-eye pair so both eyes render from (and submit with) the same
    // head viewpoint, differing only by IPD. This removes the inter-eye head-pose
    // differential (left rendered at present P, right at P+1) that SteamVR's
    // per-view reprojection amplifies into one-sided left-eye judder/tearing —
    // Virtual Desktop masks it, so it stays off there (already smooth on the
    // per-eye path). Confirmed direction by the user's both-left/both-right=smooth
    // test: identical per-eye pose = smooth, differing per-eye pose = left tears.
    // Key off the ACTUALLY-detected runtime (by name), not just the xr_runtime ini
    // flag: SteamVR can be the system default OpenXR runtime with xr_runtime=0, and
    // the lock must still engage there or the left-eye judder returns.
    if (OpenXRManager::Get().IsRuntimeSteamVR()) {
        return 1;
    }
    return g_liveControls.xrRuntime == 1 ? 1 : 0;
}

extern "C" int Get3DofMovement() {
    return g_liveControls.xr3DofMovement;
}

extern "C" float GetMotionPredictMs() {
    return g_liveControls.xrMotionPredictMs;
}

extern "C" int GetRenderPoseSubmit() {
    return g_liveControls.xrRenderPoseSubmit;
}

extern "C" int GetDepthSubmit() {
    return g_liveControls.xrDepthSubmit;
}

extern "C" int GetPoseLag() {
    return g_liveControls.xrPoseLag;
}

extern "C" float GetVrSharpness() {
    return g_liveControls.xrSharpness;
}

extern "C" float GetVrSharpmix() {
    return g_liveControls.xrSharpmix;
}

extern "C" int GetReuseLastFrameOutput() {
    return g_liveControls.xrReuseLastFrame;
}

// GetVrPairLock() removed along with the pair lock itself. g_liveControls.xrPairLock survives
// only so an existing vrport.ini keeps parsing and re-saving without losing the line.

extern "C" int GetXrRuntimeMode() {
    return g_liveControls.xrRuntime;
}

extern "C" int GetInputActionsEnabled() {
    return g_liveControls.xrInputActions != 0 ? 1 : 0;
}

extern "C" int GetMonoXQueueWait() {
    return g_liveControls.xrMonoXQueueWait != 0 ? 1 : 0;
}

extern "C" int GetSnapTurnPulseMs() {
    int v = g_liveControls.xrSnapTurnPulseMs;
    return v > 0 ? v : 30;
}

extern "C" int GetMonoDepthCapture() {
    return g_liveControls.xrMonoDepthCapture != 0 ? 1 : 0;
}

extern "C" int GetSnapTurnYawIndex() {
    int v = g_liveControls.xrSnapTurnYawIndex;
    return (v >= 0 && v <= 3) ? v : 1;
}


// Moved to src/Core/Log.cpp: the log, and the guarded dumps that read engine memory.

// ======================== TELEMETRY ========================


TelemetryData*  g_telemetry   = nullptr;   // see Core/Telemetry.hpp -- ONE object
SetterTraceData* g_setterTrace = nullptr;

volatile uintptr_t g_settingsResPtr = 0;
volatile uintptr_t g_dlssResPtr = 0;

float* GetShotShared();  // shared-mem accessor (defined below)

// MAP PIN-DRIFT FIX. The map pins slide off the background on pan/zoom because
// the game's UI projection assumes 16:9 but we force a 1:1 square resolution.
// While the world
// map is open (shared[81], set by redscript bridge SetVRMenuOpen), STOP applying
// our square-resolution override — let the game use its real 16:9 resolution for
// the map's UI projection so pins track the background correctly.
void ApplySettingsResolutionOverride(uintptr_t settingsPtr) {
    // Both straight from the launcher. The aspect-derived variant that used to sit here is
    // gone with the DLSS overrides -- nothing may re-derive a size from the runtime's
    // recommended render target any more; that is what cost 3.4 degrees of vertical field.
    const UINT forcedWidth = GetForcedWindowWidthValue();
    const UINT forcedHeight = GetForcedWindowHeightValue();

    if (!settingsPtr || forcedWidth == 0 || forcedHeight == 0) {
        return;
    }

    // World map open? Suspend the override (test).
    {
        uint32_t mapFlag = 0;
        if (float* sh = GetShotShared()) {
            mapFlag = reinterpret_cast<volatile uint32_t*>(sh)[81];
        }
        static uint32_t s_lastMapFlag = 0xFFFFFFFF;
        if (mapFlag != s_lastMapFlag) {
            s_lastMapFlag = mapFlag;
            if (g_verboseLog) {
                Log("ApplySettingsResOverride: mapFlag[81]=%u -> %s\n",
                    mapFlag, mapFlag ? "SUSPEND resolution override (map open)" : "apply square");
            }
        }
        if (mapFlag != 0u) {
            return;
        }
    }

    // VR Mod tracks the settings struct around CP2077SettingsRes; +0x18/+0x1C are the
    // active dimensions and +0x84/+0x88 are the validator targets used by the game.
    WriteU32Safe(settingsPtr + 0x18, forcedWidth);
    WriteU32Safe(settingsPtr + 0x1C, forcedHeight);
    WriteU32Safe(settingsPtr + 0x84, forcedWidth);
    WriteU32Safe(settingsPtr + 0x88, forcedHeight);
}

// Only the SETTINGS override is left. The DLSS one was removed 2026-08-03: it was AER-era,
// off by default for good reason (it broke MAIN's DLSS outright), and while it sat there
// switched off it kept a size-rederivation helper alive that later leaked into the swapchain
// path and cost vertical field of view. A knob nobody should turn is not worth its blast radius.
void ApplyKnownResolutionOverrides() {
    const uintptr_t settingsPtr = g_settingsResPtr;
    if (settingsPtr != 0) {
        ApplySettingsResolutionOverride(settingsPtr);
    }
}

// THE ENGINE'S FOV FIELD IS VERTICAL. Measured live in x64dbg on the MAIN view context: we wrote
// 94.0 (the de-canted horizontal) and the frustum came back tan(V/2) = 1.072369 -- exactly tan 47,
// i.e. the engine took our number as the VERTICAL -- with tan(H/2) = 1.002432, which is precisely
// tan(V/2) * 2064/2208, the render target's aspect. So H is derived, never set:
//
//     tan(H/2) = tan(V/2) * width / height
//
// The consequence was a four-degree mismatch: rendered H 90.14 while the submit said 94, and the
// compositor stretches whatever it is handed to fill what it was promised -- the world reads too
// large. R.E.A.L. VR writes 100.02 here on the same headset, which derives to H 94.02, matching
// what it submits.
//
// So two values, and keeping them distinct is the whole fix:
//   g_normalFovOverrideValue  the VERTICAL, i.e. what the engine's field actually receives
//   g_engineHorizontalFovDeg  the horizontal that then falls out of it -- the real rendered H,
//                             which is what the submit layer and the overlay reticle both need
volatile float g_normalFovOverrideValue = 0.0f;
volatile float g_engineHorizontalFovDeg = 0.0f;

// The FOV (degrees) the GAME actually renders the scene with, captured live by
// OnNormalFovHookCallback (native by default, or xr_force_fov). The OpenXR submit
// path reads this so the projection-layer FOV MATCHES the rendered content (an
// XrCompositionLayerProjectionView.fov must describe the frustum the image was
// rendered with, not the lens). 0 until the FOV hook first fires.
// The HORIZONTAL the engine ends up rendering, not the value written into its field. Callers --
// the OpenXR submit layer and the overlay's reticle projection -- all want the horizontal, and on
// a symmetric headset the two were the same number, which is why returning the written value
// worked until a canted one turned up.
extern "C" float GetGameRenderFovDeg() {
    const float f = g_engineHorizontalFovDeg;
    return (f > 1.0f && f < 170.0f) ? f : 0.0f;
}

// The VERTICAL the engine renders -- the value in its FOV field, symmetric about the camera axis.
// The submit layer needs it for the same reason it needs the horizontal: an OpenXR projection view
// is a promise that the given rectangle contains exactly the given frustum, and the rectangle
// contains what was rendered, not what the runtime happens to report for the panel.
extern "C" float GetGameRenderVerticalFovDeg() {
    const float f = g_normalFovOverrideValue;
    return (f > 1.0f && f < 179.0f) ? f : 0.0f;
}

// FOV overscan factor. Fixed at 1.0 (no overscan): overscan changed the game FOV
// away from the lens FOV (~103.982 on a symmetric HMD) and distorted scale.
extern "C" float GetFovOverscan() {
    return 1.0f;
}
volatile int g_menuModeValue = 0;

// Overscan factor: render (and submit) a FOV this much wider than the lens, so the
// compositor's reprojection (ATW) on head turns has rendered pixels beyond the lens
// edge to pull in -> no edge stretch. The runtime crops the wider image back to the
// lens, so the VISIBLE FOV + scale stay correct. ~1.0 = no margin = stretch on turn
// (the bug). The "body big" era accidentally had margin because the render FOV was
// far NARROWER than the submitted FOV. Tunable via xr_fov_overscan.
extern "C" float GetFovOverscan();  // defined below near the live-controls getters

// The VERTICAL FOV (deg) we want the game to RENDER = lens vertical * overscan.
extern "C" float GetTargetRenderVfovDegC();
float GetTargetRenderVfovDeg() {
    const float vfovDeg = OpenXRManager::Get().GetRuntimeVerticalFovDeg();
    if (!(vfovDeg > 1.0f && vfovDeg < 175.0f)) return 0.0f;
    float os = GetFovOverscan();
    if (!(os >= 1.0f && os <= 2.0f)) os = 1.3f;
    const float t = vfovDeg * os;
    return (t > 1.0f && t < 178.0f) ? t : vfovDeg;
}

// C-linkage wrapper so the OpenXR submit (openxr_manager.cpp) can set the submitted
// FOV to the SAME overscanned target the game renders -> render == submit, runtime
// crops to lens, ATW gets margin.
extern "C" float GetTargetRenderVfovDegC() { return GetTargetRenderVfovDeg(); }

// Moved to src/Core/CameraMath.cpp: FOV, IPD and turning a quaternion into the engine camera basis.

bool IsPlausibleCameraSpan(const float* a, const float* b) {
    if (!a || !b) return false;
    if (!IsPlausiblePositionVec4(a) || !IsPlausiblePositionVec4(b)) return false;

    const float dx = b[0] - a[0];
    const float dy = b[1] - a[1];
    const float dz = b[2] - a[2];
    const float spanSq = dx * dx + dy * dy + dz * dz;
    return spanSq < 25.0f;
}

volatile int32_t g_lastLocatePosFP[3] = {};   // world head CENTRE, fixed point 1/131072
// LATE IPD SHIFT: the per-eye stereo offset, computed (and eye-signed) in
// LocateCamera but NOT applied to the located camera there. The located camera
// stays at the head CENTER so the engine's IK/physics/VRIK see a stable,
// non-jittering head. OnFinalCameraCallback adds this shift to the final render
// camera only — post-IK, just before projection.
volatile int32_t g_lastIpdShiftFP[3] = {};
volatile float g_lastLocateQuat[4] = { 0.0f, 0.0f, 0.0f, 1.0f };  // located (HMD-injected) game-world cam quat; read by the overlay barrel crosshair

// The head orientation LocateCamera composed this frame: heading (mouse/stick) * HMD pose.
// Written by PatchCamera into BOTH cameras. Kept separate from g_lastLocateQuat, which is a
// mirror of the serialiser buffer and therefore useless once we stop writing that buffer.
// The two camera objects, cached. Identification then costs two pointer compares.
//
// The name read is the slow path and it must not be the common one: this site fires ~16.3M
// times against ~12k camera hits, so on all but a vanishing fraction of calls we would be
// dereferencing an unrelated object to learn it is not a camera. Pointer equality answers that
// without touching memory the object owns.
//
// The cache is self-healing rather than permanent: components are recreated on respawn, load
// and camera switches, so a miss simply falls through to the name read, which re-latches. That
// keeps it correct without ever needing an invalidation event to be delivered.
std::atomic<uintptr_t> g_camObjMain{0};
std::atomic<uintptr_t> g_camObjVrcam{0};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCamRebinds = 0;

volatile float g_headQuatComposed[4] = { 0.0f, 0.0f, 0.0f, 1.0f };
volatile uint32_t g_headQuatValid = 0;     // 0 while the shot-frame/native-aim skip is active

// The ENGINE's own camera orientation, snapshotted at PatchCamera BEFORE we overwrite it.
//
// This exists to break a feedback loop, and the loop is not subtle: LocateCamera derives the
// body heading from the camera's current orientation. While the write went into the
// serialiser buffer the engine refilled that buffer from its own state every frame, so the
// base was clean. Writing the camera OBJECT changes that -- next frame the base already
// contains the HMD rotation we applied, the heading absorbs its yaw, and we multiply by the
// HMD yaw again. The camera then spins up without bound from the smallest head turn and only
// stops if you turn back, which is exactly what it did.
//
// At the PatchCamera site the engine's own `movups` has already executed by the time our
// callback runs, so what we read there is the engine's value for this frame, before our
// overwrite -- the clean base the heading needs.
volatile float g_engineCamQuat[4] = { 0.0f, 0.0f, 0.0f, 1.0f };
volatile uint32_t g_engineCamQuatValid = 0;

// 1 = LocateCamera composes and PatchCamera writes (the correct split, see the comment at the
// write site). 0 = the legacy path, orientation written into LocateCamera's serialiser buffer.
// Exported so the two can be compared live without a rebuild.
// 0 = the MONO path: LocateCamera composes AND writes the orientation. 1 = the stereo-era split,
// PatchCamera writes it.
//
// Back to 0, 2026-07-30, on the observation that mono never had this twitch. The two sites sit at
// different points in the frame: PatchCamera is the gameplay tick, LocateCamera runs later, during
// render. Writing early leaves the engine's own procedural camera pass to run AFTER us, so what
// reaches the frame is its result blended over ours -- measured as 0.18 deg and 0.4 mm of change
// at frame open while the head sample, the heading and our composed quaternion were all frozen.
// Writing late overwrites that pass instead, which is exactly what mono did.
//
// The orientation is the same for both eyes, so it does not need the per-view split that the
// POSITION does -- the eye separation stays where it is, in the write callback.
// BACK TO 1. Tried at 0 (the mono path, orientation written in LocateCamera) on the reasoning
// that mono never twitched: it did not help, and it cost VRCAM its orientation entirely --
// LocateCamera writes the located buffer, and the second view's camera object never receives it.
// So the split is not optional in stereo: the orientation has to be written per view, where the
// view is known.
// ISOLATION TEST DONE, BACK TO 1. Writing BOTH orientation and position into the
// SerializeSetup buffer instead of the component -- i.e. inside the director update,
// below the component and above the blender -- left the mouse-turn trail EXACTLY as it
// was. So the trail does not come from this write, from its stage, or from anything the
// value passes through between the component and the blender. Recorded because it is a
// clean negative: the camera write is no longer a suspect, and the buffer path is not an
// option in stereo anyway (it cannot tell MAIN from VRCAM).
extern "C" __declspec(dllexport) int CyberpunkVR_CamWriteInPatch = 1;

// ---- COMPOSE AT THE WRITE SITE ------------------------------------------------------------
//
// LocateCamera publishes the HEADING only; PatchCamera multiplies it by the HMD pose and
// writes the product. The split follows how fast each part moves:
//
//   heading - mouse/stick yaw, recenter, physical-body realign. Gameplay-rate, and a value one
//             interval old is not detectable in it.
//   HMD     - the whole point. It has to be the sample belonging to the frame being built, and
//             only the write site knows when that is.
//
// Composing in LocateCamera and writing in PatchCamera made the result depend on which of the
// two happens to run first inside an interval, and nothing guarantees an order: measured, MAIN
// is written on ~85% of intervals and LocateCamera pushes on ~82%, so they disagree often.
// Whenever Patch leads, it writes the PREVIOUS interval's product -- a full frame of
// orientation lag that never catches up, and an image that does not match the pose submitted
// with it. Composing here takes the ordering out of the answer entirely.
volatile float g_headingSy = 0.0f;      // heading quaternion is (0, 0, sy, cy)
volatile float g_gamePitchRadians = 0.0f;
volatile float g_headingPitchS = 0.0f;  // pitch quaternion is (s, 0, 0, c)
volatile float g_headingPitchC = 1.0f;
volatile float g_headingCy = 1.0f;
volatile uint32_t g_headingValid = 0;   // 0 on the shot frame / native-aim mode

// The product actually written into both cameras, composed once per present interval.
//
// ONE value for both views, deliberately. Composing separately per camera would give MAIN and
// VRCAM orientations sampled at different instants -- a rotational disparity between the eyes,
// the one stereo error the brain cannot fuse. Whichever camera the engine updates first in an
// interval composes; the other writes the same product. In an interval where the engine
// updates neither, nothing changes and the two stay in agreement by construction.
//
// ALL OF THIS IS CROSS-THREAD. The instruction PatchCamera patches is reached from several
// engine job threads, so "compose once per interval" needs a compare-exchange to actually mean
// once -- otherwise two threads compose in the same interval, each publishes a different pose
// as the frame's pose, and the last one to land wins at random. And the four floats need a
// seqlock, because a reader that catches two of them from before a write and two from after
// gets a quaternion that existed at no point in time. Either would show up as an occasional
// unexplained jolt, which is the most expensive kind of bug to go looking for later.
std::atomic<uint64_t> g_camComposedForPresent{~0ull};







extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugFinalMatch   = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugFinalNoMatch = 0;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugFinalAge     = 0;   // measured depth
// How many ring entries the frame's quaternion matched. 1 = unambiguous. Above 1 means the head
// moved less than the tolerance between writes, which is the case the ordered pick exists for.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugFinalTies    = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugFinalTieHits = 0;
// 1 = label the submitted frame with the pose read back out of the engine at frame-open.
// 0 = the previous arrangement, which assumed the frame at present N used the write of N-1.
extern "C" __declspec(dllexport) int CyberpunkVR_PoseReadBack = 1;

// 1 = compose at the write site (above). 0 = the previous split, where LocateCamera composed
// and PatchCamera copied. Live-switchable so the two can be compared inside one session.
//
// LEFT AT 1. Of the four flags that were still unexamined -- this one, BindPoseToImage,
// PoseReadBack and CamFinalRowOrder -- only this one can change a rendered pixel; the other three
// decide which pose LABEL is attached to a frame that has already been drawn. But the argument for
// moving it (the aim epoch advances at display rate while the camera is written at game rate, so
// the composes-per-frame count alternates) requires the two rates to differ, and the twitch is
// there at 90+ fps in mono as well. Rate mismatch is not the mechanism. Not touched.
extern "C" __declspec(dllexport) int CyberpunkVR_CamComposeAtWrite = 1;
// 1 = locate the head afresh at the camera write, aimed at the predicted display time of the
// frame being built (the RealVR arrangement). 0 = read the cached atomics the frame-loop thread
// refreshes, whose age relative to the write wanders frame to frame.
extern "C" __declspec(dllexport) int CyberpunkVR_PoseLocateAtWrite = 1;
// 1 = LocateCamera's translation and PatchCamera's orientation share ONE head sample per frame
// (AcquireFrameHeadSample). 0 = the previous arrangement, position from the smoothed cache and
// orientation from a separate locate. Live-switchable so the difference can be felt directly.
extern "C" __declspec(dllexport) int CyberpunkVR_OneSamplePerFrame = 1;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPoseLocatedAtWrite = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPoseFromCache = 0;
// Defined in openxr_frameloop.cpp -- how many presents ahead the frame being built is shown.
extern "C" __declspec(dllexport) int CyberpunkVR_EnginePipelineDepth;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCamComposed   = 0;
// How often VRCAM, not MAIN, was the first camera the engine updated in an interval. Non-zero
// means the order really is not fixed, which is the whole reason composition moved here.
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCamVrcamFirst = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCamNoHmd      = 0;

// Which thread each stage runs on -- this is what decides whether PoseFrameLag should be 0 or
// 1, and it has never been established.
//
// If the camera write happens on the SAME thread as Present, the write and the recording of
// the frame it belongs to are serialised: the frame goes out at the next present, so a write
// stamped with interval N belongs to present N+1 and the lag is 0. If it happens on a
// different (simulation) thread, that thread runs ahead of the render thread and the frame
// carrying the write is presented one or more intervals later -- lag >= 1. Guessing between
// the two is a coin flip that costs a whole session, so both ids are exported and can be read
// straight out of the process.
// ---- HEAD TRANSLATION, SHARED BY BOTH VIEWS ------------------------------------------------
//
// The head's world-space displacement for this frame: HMD translation rotated into the game's
// heading, plus the Tracking/Camera offsets and the calibration bakes. LocateCamera is the only
// place that can build it (it has the flat heading, the bakes and the vehicle/menu rules), but
// it was also the only place that APPLIED it -- straight into the located camera buffer, which
// is MAIN's alone. VRCAM never saw a single millimetre of it, which is why the second eye sat
// welded to the head while the first one correctly moved away from it, and why the
// Tracking/Camera offset sliders appeared to do nothing to VRCAM.
//
// The three mods worth copying all solve this the same way and it is worth writing down,
// because it is the shape our code was missing rather than a detail:
//
//   Crysis VR    view = base * eye              (base = entity pos + yaw only; eye = FULL HMD
//   (fholger)                                    transform, rotation AND translation)
//   Far Cry VR   view = base * head * eye       (base = VR base pos + yaw only)
//   Portal 2 VR  origin = setupOrigin + hmdPosRelative, then +/- right*ipd/2 per eye
//
// In every one of them the head translation is applied ONCE, to a value both eyes share, and
// the eyes differ by the lateral IPD term and nothing else. Published here in the engine's own
// int32 fixed-point (x131072) so the write site can add it to a component position directly.
std::atomic<int32_t> g_headDeltaFP[3] = {};
std::atomic<uint32_t> g_headDeltaValid{0};

extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugTidPatchCam = 0;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugTidLocateCam = 0;

// ---- THE PER-VIEW WRITE SITE (mono) ---------------------------------------------------------
//
// 1 = drive both views from CRenderNode_PrepareSceneRendering's camera fix-up (see
// OnFinalCameraCallback), which is per-view, runs at frame open, and writes the very object the
// view-matrix bake reads. 0 = the current arrangement, where PatchCamera writes the placed
// component and VRCAM needs a separate translation patch.
//
// OFF, and the reason is worth keeping: FINAL CAMERA IS A CONSUMER, NOT THE SOURCE.
//
// Tried and rejected on evidence. Writing the render camera here rotates the rasterised near
// geometry correctly, but everything the engine had ALREADY derived from the camera earlier in
// the frame -- culling frustum, shadow-cascade setup, distant/imposter selection, the previous
// frame's matrices feeding TAA/DLSS -- stays on the engine's un-written value. The result on
// screen is exact and diagnostic: near objects stay world-locked while distant geometry and
// shadows drag with the head, because half the frame is built from one camera and half from
// another.
//
// The chain is component transform -> view producer (sub_140252034 / sub_140293978) -> render
// camera (ctx+0x18) -> view matrices (sub_140788A9C). PrepareSceneRendering's fix-up and
// SetStreamlineConstants both sit BELOW the producer, so both are downstream of the decisions
// that already used the camera. Only a write at the component -- PatchCamera -- is upstream of
// all of them, which is why that is where the engine's own writer lives and where RealVR hooks.
//
// Counters from the attempt, for the record: ViewCamMain 6192, ViewCamVrcam 5625 (both views DO
// reach the site once the view test used the dispatcher's tags instead of a component-name hash),
// ViewCamOther 0 (there are no extra views here at all).
extern "C" __declspec(dllexport) int CyberpunkVR_CamWriteInFinal = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugViewCamMain = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugViewCamVrcam = 0;
// Views that are neither eye: distant/imposter, reflection, shadow. Counted separately because
// how many there are per frame decides whether they can be the cause of anything.
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugViewCamOther = 0;
// 1 = give every view in the image the head orientation (see the write site). 0 = only the two
// eye views, which left distant geometry and shadows turning with the head.
extern "C" __declspec(dllexport) int CyberpunkVR_CamFinalViewScope = 1;
// The dispatcher's own view tags -- the same pair the VRCAM capture pipeline runs on.
extern "C" __declspec(dllexport) int CyberpunkVR_IsVrcamViewActive();
// 1 = give VRCAM the same head translation MAIN gets. Live-switchable to isolate it.
extern "C" __declspec(dllexport) int CyberpunkVR_VrcamHeadTranslation = 1;
// 1 = hold the gamepad LT back on foot with empty hands, so striking the smoking lighter does not
// also pull the camera into aim-zoom. 0 = vanilla LT everywhere, for anyone not using that mod.
// Driving is never gated: no weapon is equipped in a car, and that is where LT is the brake.
extern "C" __declspec(dllexport) int CyberpunkVR_LtLighterGate = 1;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugVrcamPosWrites = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMainPosWrites = 0;
// 1 = both cameras get the head translation in the COMPONENT (PatchCamera). 0 = the old
// split, where MAIN took it in LocateCamera's serialised buffer and was therefore weighted
// by the blender while its orientation was not. See the use site.
// 1 = MAIN takes the head translation in the COMPONENT, the same way VRCAM always has.
// 0 = the old split, where MAIN took it in LocateCamera's serialised buffer.
//
// The component is the right home: the buffer entry is a blender CameraSetup, and the blender
// multiplies every field by the camera's weight -- so MAIN's translation was weighted while its
// orientation, which travels through the component, was not. On any two-camera transition the two
// disagreed. Through the component both are weighted identically.
//
// (A first attempt at this looked like it had failed -- the view rode the character's head. That
// was an unrelated regression in the same session: the VRIK cleanup had replaced HALF of a
// coherent pair, taking hmdRel from a fresh head sample while the controllers it un-rotates came
// from an older publication. The argument that settled it is simple and was the user's: the
// ORIENTATION reaches the blender through this very field and works, so the field is read after we
// write it, and position cannot be the exception.)
// 1 = MAIN takes the head translation in the COMPONENT, the same way VRCAM always has; 0 = the
// old split, where MAIN took it in LocateCamera's serialised buffer.
//
// The component is the right home: the buffer entry is a blender CameraSetup and the blender
// multiplies every field by the camera's weight, so MAIN's translation used to be weighted
// while its orientation -- which travels through the component -- was not. Through the
// component both are weighted identically.
//
// Note for anyone reading a symptom here: head translation, the camera bake and the
// Tracking/Camera sliders are ONE vector (worldDelta), so if the view ever stops following the
// head, the offset sliders go dead in the same instant. Flip this live to separate that from
// anything in the VRIK path: python vrprobe.py translation 0|1.
extern "C" __declspec(dllexport) int CyberpunkVR_HeadTranslationInPatch = 1;

// 1 = take the body heading from the component's PRE-WRITE world rotation at the write site,
// instead of the g_headingSy/Cy pair LocateCamera publishes. Those are published downstream of
// this write (LocateCamera runs inside the blender), so the cached pair is one frame old and the
// camera lags the body by heading-change-per-frame -- 4.2 deg at a 300 deg/s mouse turn. See the
// use site in PatchCamera.
extern "C" __declspec(dllexport) int CyberpunkVR_HeadingFromPreWrite = 1;

// 1 = rebuild the head translation from the frame's OWN head sample using the recipe
// LocateCamera publishes (g_anchorOff / g_anchorCy / g_anchorSy / g_anchorScale), instead of
// reading g_headDeltaFP -- which LocateCamera computes AFTER this write and therefore belongs
// to the previous frame. Same class of defect as the stale heading, same remedy.
extern "C" __declspec(dllexport) int CyberpunkVR_DeltaFromFreshSample = 1;
// How far AHEAD of the tick heading to aim the camera, in ticks. 0 = the tick heading, which is
// what the body's tick is; the body itself is drawn with an INTERPOLATED entity transform, so
// somewhere between 0 and 1 is the value that matches what is on screen. Not guessable from
// here (the serialise buffer is a verbatim component copy, so there is no render-rate heading
// to compare against) -- turn the knob until the trail on a fast mouse turn disappears, and
// that reading is the answer. Read CyberpunkVR_DebugHeadingStepDeg to see the scale involved.
extern "C" __declspec(dllexport) float CyberpunkVR_HeadingLeadFrames = 0.0f;
// Peak |heading change| per composition, in degrees -- the size any phase error can have.
// Cleared by whoever reads it.
extern "C" __declspec(dllexport) float CyberpunkVR_DebugHeadingStepDeg = 0.0f;
// The two headings the write site can see, in degrees: the one extracted from the component's
// pre-write world rotation, and the cached pair LocateCamera publishes. Live, so a constant
// value is immediately visible as constant.
extern "C" __declspec(dllexport) float CyberpunkVR_DebugHeadingPreWriteDeg = 0.0f;
extern "C" __declspec(dllexport) float CyberpunkVR_DebugHeadingCachedDeg = 0.0f;

// THE BODY-YAW CENSUS. Peak-held per window, cleared by the reader in OpenXRPresent.
// lag  = |view heading - entity world yaw| in degrees: how far behind the drawn body is.
// step = |entity yaw change| per fresh solve: the turn rate the lag has to be read against.
// hips = |hips MODEL yaw change| per fresh solve: zero means no bone carries the turn and the
//        yaw lives entirely in the entity transform. Sampled in AnimPose, where the solve is.
extern "C" __declspec(dllexport) float CyberpunkVR_DebugBodyYawLagDeg = 0.0f;
extern "C" __declspec(dllexport) float CyberpunkVR_DebugBodyYawStepDeg = 0.0f;
extern "C" __declspec(dllexport) float CyberpunkVR_DebugHipsYawStepDeg = 0.0f;

// THE PHASE OF THE BODY YAW against the animation batch. Peak-held per window, cleared by the
// reader. yawToSolve = ms between sub_140336390 storing the yaw and our fresh solve reading it:
// near zero means the yaw of this frame exists before the pose is baked, near a frame period
// means it does not. lagWrite = the same disagreement in degrees, our heading against the yaw
// that site actually stored -- no CET push in that path. See src/Hooks/BodyYawCensus.cpp.
extern "C" __declspec(dllexport) float CyberpunkVR_DebugYawToSolveMaxMs = 0.0f;
extern "C" __declspec(dllexport) float CyberpunkVR_DebugYawToSolveMinMs = 0.0f;
extern "C" __declspec(dllexport) float CyberpunkVR_DebugYawLagWriteDeg = 0.0f;

// 1 = the solve takes its world->model yaw from the ENGINE body yaw (published by the write site
// sub_140336390, see src/Hooks/BodyYawCensus.cpp) instead of the view packet heading. Measured:
// the yaw is stored 0.22-0.79 ms BEFORE the animation batch, while the packet heading is a frame
// old, and the two differ by 5-10 deg on an ordinary mouse turn. Live flag so the two can be
// compared without a rebuild.
extern "C" __declspec(dllexport) int CyberpunkVR_VrikYawFromEngine = 1;

// 1 = the VIEW takes its yaw from the ENGINE body yaw (what the mouse and the stick produce)
// rather than from the camera component, which inherits the entity yaw and therefore carries the
// body-follow offset. This is what makes "the body turns, the camera does not" possible at all --
// see the use site in PatchCamera. It also drops a frame of age from the heading.
extern "C" __declspec(dllexport) int CyberpunkVR_ViewYawFromEngine = 1;
// Use a coherent relative camera/entity snapshot for the model-space anchor and the same latched XR
// head sample as PatchCamera for its orientation.  See VRIK_ComputeCamModel; g_lastLocate* is too
// late in the frame to pair with the entity transform consumed by animation.
extern "C" __declspec(dllexport) int CyberpunkVR_VrikTransformsFromPlugin = 1;
// Remove the final script clock from VRIK transforms: LocateCamera's preceding frame is paired with
// the preceding engine entity transform by BodyYawFollowTick and published atomically.
extern "C" __declspec(dllexport) int CyberpunkVR_VrikNativeFramePair = 1;
// 1 = while MOUNTED, the solve takes its world->model frame from the Lua pair, whose entity
// quaternion is the FULL entity world orientation, instead of the native pair's Rz(yaw)
// reconstruction. Seated in a car the body pitches and rolls with the shell, so a yaw-only frame is
// wrong by a quantity that changes every frame the car moves -- the jitter. On foot the entity is
// upright, yaw is the whole of it, and the native pair's engine clock is the better source, so this
// changes nothing there. 0 restores the previous behaviour.
extern "C" __declspec(dllexport) int CyberpunkVR_VrikVehicleFullEntityQuat = 1;
// 1 = while MOUNTED, the play-space anchor is rotated by the yaw the VIEW was composed with, instead of
// by the body's own forward.
//
// MEASURED FROM THE SYMPTOM: the jitter is present only while the car is TURNING -- not parked, not
// driving straight -- and it is visible on the flat monitor, i.e. in the image this port composes. That is
// the signature of the head offset being taken into the world by a yaw on a slower clock than the frame:
// each render the car has turned further than the yaw has, so the eye sits slightly wrong by an amount
// proportional to the turn rate. LocateCamera used the body's forward (entity/tick clock) while the view
// came from the camera's pre-write quaternion (assembled per rendered frame). One clock instead of two.
//
// An earlier attempt put both on g_VREntityQ*, the Lua-pushed entity quaternion -- the coarsest clock of
// the three -- and made it worse. 0 restores the body-forward behaviour.
extern "C" __declspec(dllexport) int CyberpunkVR_VehicleAnchorFromViewYaw = 1;
// TEST BUILD: 0 = our composed orientation is NOT written while mounted, so the engine's own vehicle
// camera stands. The head stops turning the view in a car, which is why this is a test rather than a
// setting -- it exists to answer one question. Jitter gone at 0: our write is in a fight with the
// camera's bound-forward constraint. Jitter still there: the orientation was never what moved.
// PROVEN BY THE TEST BUILD: with this at 0 -- our composed orientation not written while mounted --
// the in-vehicle jitter disappeared entirely. So the jitter IS our write against the game's own
// vehicle-camera heading reset, and not the frame rate, not the yaw source and not the anchor.
//
// Back at 1 because 0 also stops the head turning the view in a car, which is not a shippable
// trade. The cure is to take the game's heading reset out of the loop rather than to stop writing:
// fppCameraParamSets.Vehicle carries headingLocked, headingResetSpeed, headingResetTimeout,
// headingResetOnlyWhenMoving, normalizeYaw and the yaw/pitch rubber band -- and
// headingResetOnlyWhenMoving alone explains why a parked car is clean.
extern "C" __declspec(dllexport) int CyberpunkVR_CamWriteOrientInVehicle = 1;
// The yaw PatchCamera actually composed the view with, published at the instant it is used. Read by
// LocateCamera in the same frame: PatchCamera writes the camera, LocateCamera runs downstream of it inside
// the blender, so this is never a cached value.
// 1 = a frame that did not claim the aim epoch still gets THIS frame's world yaw, by turning the
// published composition through the yaw it missed. Measured need: 4-12% of rendered frames share an
// epoch with the previous one and were writing that frame's orientation -- correct for the head,
// stale for the world. 0 restores the previous behaviour.
extern "C" __declspec(dllexport) int CyberpunkVR_YawCatchUpOnSharedEpoch = 1;
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugYawCaughtUp = 0;
volatile float g_viewYawUsedRad = 0.0f;
volatile int   g_viewYawUsedValid = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugVrikNativePairUsed = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugVrikLuaPairFallback = 0;
// The head bone follows the freshest camera sample, while arm/controller math keeps the stable
// rotation from the coherent camera/entity push. Set to 0 live to reproduce the mixed-frame arm path
// without also disabling the phase-coherent body anchor.
extern "C" __declspec(dllexport) int CyberpunkVR_VrikSplitHeadHandRot = 1;
// 1 = the shoulder anchor and the arm frame come from the avatar's own bones (neck for the girdle,
// root->head and the shoulder line for the axes) instead of from the camera. With the camera they
// follow the HEAD -- the elbows swing when you look around -- and after the camera-onto-head bake
// they moved back with it by the baked (0.093, -0.428) and dragged the chest and armpit along.
extern "C" __declspec(dllexport) int CyberpunkVR_VrikArmAnchorFromBody = 1;
// 1 = the elbow policy at the end of VRIK_SolveArm: blend toward a down/back rest direction as the
// hand approaches the shoulder (VRArmIK's fixed-elbow-near-shoulder rule, its own constants), and
// cap the elbow height at min(wrist, shoulder) unless the hand is raised above the shoulder. Both
// keep the elbow close to the body and out of the poses a free IK likes and a human never uses.
extern "C" __declspec(dllexport) int CyberpunkVR_VrikElbowPolicy = 1;
// Distance between where the solved hand lands in the WORLD and where the controller actually is,
// millimetres, right hand. Zero means the hand is on the controller. See the compute site in
// AnimPose: it is the only way to tell which frame is wrong when "the hands ride the body".
extern "C" __declspec(dllexport) float CyberpunkVR_DebugHandWorldErrMm = 0.0f;
// How far the SOLVED wrist ended up from the target it was given (mm), and how far that target sits
// from the shoulder as a fraction of arm length. The pair separates "the target is wrong" from "the
// arm could not reach it" -- see the compute site right after VRIK_SolveArm.
extern "C" __declspec(dllexport) float CyberpunkVR_DebugHandMissMm = 0.0f;
extern "C" __declspec(dllexport) float CyberpunkVR_DebugHandReachRatio = 0.0f;
// The ENGINE's camera position, fixed point, as it stood before PatchCamera added the head
// displacement. The body and the shoulders hang off this; reconstructing it from the view minus the
// delta put the physical head motion back in with the wrong sign. See the publish site.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_EngineCamPosFP[3] = { 0, 0, 0 };
extern "C" __declspec(dllexport) int     CyberpunkVR_EngineCamPosValid = 0;
// Horizontal distance from the character's origin to the FPP camera -- the radius the body
// follower's heading sweeps the view along. Sizes the "head and body are not in the same place
// after a turn" residual: slide = radius * 2*sin(realign/2).
extern "C" __declspec(dllexport) float   CyberpunkVR_DebugCamMountM = 0.0f;
// OFF BY DEFAULT, and that is deliberate: this MOVES THE CAMERA.
//
// The engine's heading sweeps the FPP camera along the mount circle when the body follower turns, so
// the view slides sideways even though the player's real head did not move. Taking that back out means
// writing the camera position -- the one thing that has to be earned rather than assumed, and the
// first version of it broke the game outright by reading back its own write and compounding across
// passes. This version cannot compound (it is a function of angles and a learned mount vector), but
// whether the slide or the correction feels worse is a judgement, not a derivation, so it ships off
// and can be flipped live without a restart.
extern "C" __declspec(dllexport) int     CyberpunkVR_CamMountCompensate = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugViewYawFromEngine = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugHeadingLedComps = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugDeltaRebuilt = 0;
// 1 = put the eye separation into the component's WORLD POSITION (component+0xE0), above the view
// producer, so culling / shadows / distant pass / motion vectors all see the eye they are drawn
// for. 0 = do not separate the cameras at all.
extern "C" __declspec(dllexport) int CyberpunkVR_IpdInWorldPos = 1;
extern "C" int CyberpunkVR_MainIsRightEye;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugIpdWorldWrites = 0;
// The legacy write into component+0x100/0x110 ("posA/posB"). OFF: measured to have no effect on
// the rendered viewpoint -- the two render cameras stayed 23 micrometres apart with it enabled.
// Kept switchable only so the old behaviour can be restored in one session if something depended
// on those fields for a reason we have not found.
extern "C" __declspec(dllexport) int CyberpunkVR_IpdInPosAB = 0;
// Counts how often the camera write arrives on a DIFFERENT thread than the previous one. A
// value that stays near 1 means the site is effectively single-threaded for cameras; one that
// climbs with the frame count means it is not, and everything the write site touches has to be
// safe against that -- which is why the composition below is a compare-exchange and the
// quaternion a seqlock rather than four plain stores.
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCamThreadSwitches = 0;

volatile uint32_t g_lastLocateSeq = 0;
volatile uint32_t g_renderedSeq = 0;

extern "C" uint32_t GetRenderedCameraSeq() {
    return g_renderedSeq;
}


extern "C" int GetMenuMode() {
    return g_menuModeValue;
}

void NormalizeQuat(float& x, float& y, float& z, float& w) {
    const float lenSq = x * x + y * y + z * z + w * w;
    if (lenSq <= 0.000001f) {
        x = 0.0f; y = 0.0f; z = 0.0f; w = 1.0f;
        return;
    }

    const float invLen = 1.0f / sqrtf(lenSq);
    x *= invLen;
    y *= invLen;
    z *= invLen;
    w *= invLen;
}

void MulQuat(float ax, float ay, float az, float aw,
                    float bx, float by, float bz, float bw,
                    float& ox, float& oy, float& oz, float& ow) {
    ox = aw * bx + ax * bw + ay * bz - az * by;
    oy = aw * by - ax * bz + ay * bw + az * bx;
    oz = aw * bz + ax * by - ay * bx + az * bw;
    ow = aw * bw - ax * bx - ay * by - az * bz;
}

// Shot-decouple bridge: publish the LOCATED camera pointer (rbxPtr -- the struct where
// we inject HMD, and the one the bullet reads) + a controller-aim quaternion built in the
// EXACT same convention as the camera quat, to the shared memory the RED4ext plugin reads.
// The plugin's ShotSnap hook then brackets the located camera around the player shot:
// write controllerAimQuat -> bullet flies down the controller; restore HMD -> view stays.
// Layout: 256 floats -- FULL slot map + numbering rules live in src/shared_slots.h.
// This bridge uses [50] valid-seq, [51]/[52] locatedCamPtr lo/hi, [53..56] controllerAimQuat.
static float* g_shotShared = nullptr;
static HANDLE g_shotSharedHandle = nullptr;
float* GetShotShared() {
    if (!g_shotShared) {
        g_shotSharedHandle = OpenFileMappingA(FILE_MAP_ALL_ACCESS, FALSE, "CyberpunkVR_Hands_Shared");
        if (!g_shotSharedHandle)
            g_shotSharedHandle = CreateFileMappingA(INVALID_HANDLE_VALUE, NULL, PAGE_READWRITE, 0, 1024, "CyberpunkVR_Hands_Shared");
        if (g_shotSharedHandle)
            g_shotShared = static_cast<float*>(MapViewOfFile(g_shotSharedHandle, FILE_MAP_ALL_ACCESS, 0, 0, 1024));
    }
    return g_shotShared;
}


// ============================================
// VARIABILI GLOBALI PER LA CACHE
// ============================================
RED4ext::CProperty* g_mountedVehicleProp = nullptr;
RED4ext::CProperty* g_isAimingProp = nullptr;
RED4ext::CProperty* g_equippedWeaponProp = nullptr;
RED4ext::CProperty* g_sceneTierProp = nullptr;
RED4ext::CBaseFunction* g_isDriverFunc = nullptr;
bool g_isRTTIInitialized = false;



// ============================================
// INIZIALIZZAZIONE RTTI
// ============================================
void InitializeMountedVehicleCache() {
    if (g_isRTTIInitialized) return;

    auto rtti = RED4ext::CRTTISystem::Get();
    auto playerPuppetCls = rtti->GetClass("PlayerPuppet");
    
    if (playerPuppetCls) {
        g_mountedVehicleProp = playerPuppetCls->GetProperty("mountedVehicle");
        g_isAimingProp = playerPuppetCls->GetProperty("isAiming");
        g_equippedWeaponProp = playerPuppetCls->GetProperty("equippedRightHandWeapon");
        // The cutscene tier, verified by RTTI dump rather than assumed: PlayerPuppet has
        // `sceneTier : GameplayTier`. The blackboard route the upstream PR used
        // (GetAllBlackboardDefs -> PlayerStateMachine -> GetLocalInstanced -> GetInt) is four
        // RTTI calls to reach the same number this reads in one.
        g_sceneTierProp = playerPuppetCls->GetProperty("sceneTier");

        if (g_mountedVehicleProp) {
            std::cout << "[VR] Found property: mountedVehicle (type: " 
                      << g_mountedVehicleProp->type->GetName().ToString() << ")" << std::endl;
        } 

        if (g_isAimingProp) {
            std::cout << "[VR] Found property: isAiming" << std::endl;
        }

        if (g_equippedWeaponProp) {
            std::cout << "[VR] Found property: equippedRightHandWeapon" << std::endl;
        }

    }

    // THE DRIVER SEAT. The class is registered lower-case ("vehicleComponent") in some builds and
    // capitalised in others; ask for both rather than guess.
    for (const char* cls : { "VehicleComponent", "vehicleComponent" }) {
        if (g_isDriverFunc) break;
        if (auto c = rtti->GetClass(cls)) g_isDriverFunc = c->GetFunction("IsDriver");
    }
    Log("[VR] VehicleComponent::IsDriver %s -- wheel grab is %s\n",
        g_isDriverFunc ? "resolved" : "NOT FOUND",
        g_isDriverFunc ? "driver-seat only" : "allowed in any seat (fallback)");

    g_isRTTIInitialized = true;
}


uint64_t g_locateCameraHits = 0;
bool g_isInVehicle = false;
std::atomic<bool> g_isDriving{false};
std::atomic<int> g_sceneTier{0};
bool g_isAiming = false;
bool g_hasWeaponEquipped = false;
// [dx-win]/[jerk] diag: ENGINE located camera captured at callback entry (pre-overwrite).
float g_dbgEntryYaw = 0.0f, g_dbgEntryPosX = 0.0f, g_dbgEntryPosY = 0.0f, g_dbgEntryPosZ = 0.0f;
// [jerk] diag: the FOV the game LAST TRIED to set (pre-override) + the camera state
// pointer, so the jerk window can check for a sprint FOV boost (render zoom).
void* volatile g_dbgFovCamState = nullptr;
uint64_t g_patchCameraHits = 0;

// CName of the player's own camera component, measured live: cname_hash("camera").
// The camera object is an Entity/IPlacedComponent and carries its component name at obj+0x40,
// so this is a per-instance identity that costs one load -- no view plumbing, no
// first/last/most-frequent guessing, and stable across launches because it is a name hash.
static constexpr uint64_t kCamNameMain = 0x6FCFDF926F11594Eull;
// THE DEVICE'S OWN CAMERA, which is what a surveillance camera hands the view to. Its component is
// named `cameraComponent` -- read off the live SurveillanceCamera through the bridge, not guessed -- and
// the hash is computed with the same function the VRCAM name uses, so a mistyped literal cannot silently
// classify nothing (every wrong guess at this has been silent, which is the note above this block).
extern "C" __declspec(dllexport) int32_t CyberpunkVR_DeviceCamFollow = 1;
// Writing the HEAD POSE into the device camera. Off, and this is a measurement rather than caution: with
// it on the view started aimed at a wall and MAIN's frames jumped about while VRCAM's did not, which is
// what a fight with the camera mixer looks like -- the player's camera and the device's are both active
// during a takeover and the blender weights them. Getting the second eye onto the lens does not need it.
// BACK TO 0. With it on the view aimed at a wall, blinked, the FOV did not match between the eyes and
// VRCAM juddered on head turns -- four symptoms of one cause: MAIN is not a write at this site, it is
// a chain (LocateCamera composing and PUBLISHING the pose label the compositor reprojects against, the
// FOV override, the located buffer, FinalCamera), and a device camera was given only the write. Head
// steering inside a surveillance camera needs that whole chain pointed at it, which is a piece of work
// and not a knob. Off, the eyes both look along the lens and nothing artefacts.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_DeviceCamOrient = 1;
// THE LENS HEADING, split into yaw and pitch because that is the shape the composition takes
// (R_z(yaw) * R_x(pitch) * HMD, exactly as MAIN composes its body heading). Handing it a full mount
// quaternion instead carried the mount's roll into the product, and the head pose then arrived in a
// tilted frame. Measured on the live camera: 9.4 deg of pitch and EXACTLY no roll -- its right vector
// reads (-0.7675, 0.6411, -0.0000) -- so yaw plus pitch describes the mount completely.
float g_devCamAimYaw = 0.0f;
float g_devCamAimPitch = 0.0f;
std::atomic<int> g_devCamAimValid{0};
// The camera's authored FOV, so the object can be handed back exactly as it was found.
float g_devCamFovOrig = 0.0f;
std::atomic<int> g_devCamFovSaved{0};
// What we last wrote into that camera, and the answer both eyes then use. The first is what makes a
// per-frame base refresh safe: the field is only believed to be the engine's while it differs from this.
float g_devCamLastWritten[4] = { 0.0f, 0.0f, 0.0f, 0.0f };
float g_devCamViewQuat[4] = { 0.0f, 0.0f, 0.0f, 1.0f };
std::atomic<int> g_devCamViewValid{0};
// THE GATE AND THE TARGET, published from the script side (VRRemoteCamera in src/Natives/RemoteCamera.cpp).
// Nothing is followed until both are set, which is why a stray camera in the world can no longer be
// picked up: the name is not the identity, the position is.
std::atomic<int> g_remoteCamOn{0};
std::atomic<int32_t> g_remoteCamPosFP[3] = {};
// How close a cameraComponent has to sit to the published camera to be believed. The published point is
// the ENTITY's origin and the component sits at the lens, so this is not a few centimetres; cameras in
// the game stand metres apart, so 1.5 m separates them without being tight enough to miss the mount.
static constexpr float kRemoteCamTolM = 1.5f;
extern "C" __declspec(dllexport) unsigned int CyberpunkVR_DebugPatchCamDevice = 0;
static std::atomic<uintptr_t> g_camObjDevice{0};
// The clock of the last write to such a camera. There is no polling anywhere: this stamp IS the state,
// and it clears itself. Script systems could answer the question directly but the periodic poll in this
// plugin runs on the worker thread, where calling into the script VM is not safe.
std::atomic<unsigned long long> g_deviceCamLastMs{0};
// THE CAMERA'S OWN AIM AND PLACE, latched once per takeover.
//
// The base orientation cannot be re-read every frame: we overwrite that quaternion, so reading it back
// would compose the head pose onto our own previous output and the view would wind up. Latched on the
// first write to the camera and held until the takeover ends -- which is why the staleness test below
// invalidates it rather than any timer.
float g_devCamBase[4] = { 0.0f, 0.0f, 0.0f, 1.0f };
std::atomic<int> g_devCamBaseValid{0};
// And its world position, in the same fixed point the component stores (1/131072 m), so the second eye
// can be placed at the lens instead of at the player.
std::atomic<int32_t> g_devCamPosFP[3] = {};
std::atomic<int> g_devCamPosValid{0};
// One stamp, and the place the latch is dropped when the takeover has been away. Anything longer than
// the liveness window means this is a fresh entry, and the camera's aim and place must be taken again.
static void StampDeviceCam() {
    const unsigned long long now = GetTickCount64();
    const unsigned long long prev = g_deviceCamLastMs.exchange(now, std::memory_order_relaxed);
    if (prev == 0 || (now - prev) >= 300ull) {
        // ONLY the aim refresh. This used to clear g_devCamPosValid and g_devCamBaseValid too, which
        // meant any frame where the device camera happened not to be patched for 300 ms dropped the
        // second eye back to the PLAYER's position and its head translation -- a whole-body jump for one
        // frame, produced by a timer rather than by anything real. Those flags are cleared where they
        // belong: when control is released, by the native that owns the gate.
        //
        // Zeroing what we last wrote is what forces the lens aim to be re-read on the next patch.
        // Clearing g_devCamAimValid instead would leave one frame with no base at all, and one frame
        // with no base is a wall.
        for (int i = 0; i < 4; ++i) g_devCamLastWritten[i] = 0.0f;
    }
}

// Is this the camera the script side named? Read from the component's own world position at +0xE0, in
// the same fixed point everything else in the camera path uses.
static bool DeviceCamPositionMatches(uintptr_t obj) {
    const uintptr_t posAddr = obj + 0xE0;
    int32_t p[3] = {};
    for (int i = 0; i < 3; ++i) {
        uint32_t v = 0;
        if (!ReadU32Safe(posAddr + i * 4, &v)) return false;
        p[i] = static_cast<int32_t>(v);
    }
    const float k = 1.0f / 131072.0f;
    float d2 = 0.0f;
    for (int i = 0; i < 3; ++i) {
        const float d = (p[i] - g_remoteCamPosFP[i].load(std::memory_order_relaxed)) * k;
        d2 += d * d;
    }
    return d2 <= (kRemoteCamTolM * kRemoteCamTolM);
}

bool DeviceCamActive() {
    if (!CyberpunkVR_DeviceCamFollow) return false;
    if (!g_remoteCamOn.load(std::memory_order_relaxed)) return false;
    const unsigned long long t = g_deviceCamLastMs.load(std::memory_order_relaxed);
    if (t == 0) return false;
    return (GetTickCount64() - t) < 300ull;
}
extern "C" unsigned long long CyberpunkVR_VrcamCamNameHash();   // stereo/sync_stereo.cpp

extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPatchCamMain  = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPatchCamVrcam = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPatchCamOther = 0;

// 0 = not a camera we drive, 1 = MAIN (the player's FPP camera), 2 = VRCAM.
//
// WHY THE OBJECT AND NOT THE VIEW
//
// This hook site is NOT camera-specific. Measured live, it is the generic
// entIPlacedComponent world-transform writer: it fires for Entity/AnimatedComponent,
// Entity/SlotComponent and Entity/IPlacedComponent alike, 59k+ times in seconds. Writing the
// head pose on every call means writing it into animated components and slots -- which is the
// "world slides and the weapon drags with the head" failure, not a side effect of it.
//
// It is still the RIGHT site: the surrounding code writes the component's own store --
// [rsi+0xE0..0xE8] world position as int32 fixed-point, [rsi+0xF0] the orientation quaternion
// -- which is what the rest of the frame reads. LocateCamera by contrast patches a serialised
// COPY that the engine then partly refills behind us.
//
// Both cameras derive from entIPlacedComponent (dumped live: gameFPPCameraComponent name
// "camera", entRenderToTextureCameraComponent name "vrcam_<W>x<H>"), so both pass through
// here, and the component NAME is what tells them apart.
//
// THE OFFSET IS DISCOVERED, NOT ASSUMED
//
// Every guess at where that CName sits has been wrong (+0x40 holds a pointer, +0x48 a value
// that is identical across unrelated components), and a wrong offset here is silent: it
// classifies nothing and the cameras simply never track. So instead of hard-coding it, the
// first object whose first 0x80 bytes contain one of the two hashes we already know teaches us
// the offset, and it is latched and logged. Self-calibrating, and it survives a patch that
// shifts the layout.
static std::atomic<int> g_camNameOffset{-1};

// HAND THE CAMERA BACK. Its fov was raised from the authored value to the one the headset needs, and
// that is a change to a world object, so it is undone when control is released. Called from the
// VRRemoteCamera native, i.e. off the render path, on the tick that sees the takeover end.
//
// The pointer is validated before anything is written through it: the entity can be unloaded between the
// last patch and the release, and a blind write would land in freed memory. The check is the same one the
// classifier trusts -- the component's own CName at the calibrated offset.
void DeviceCamRestoreFov() {
    if (!g_devCamFovSaved.exchange(0, std::memory_order_acq_rel)) return;
    const uintptr_t obj = g_camObjDevice.load(std::memory_order_relaxed);
    if (!obj || obj < 0x10000) return;
    const int off = g_camNameOffset.load(std::memory_order_acquire);
    if (off < 0) return;
    uint64_t name = 0;
    if (!ReadU64Safe(obj + off, &name)) return;
    if (name != cvr::cname_hash("cameraComponent")) return;
    float cur = 0.0f;
    if (!ReadFloatSafe(obj + 0x128, &cur)) return;
    if (!(g_devCamFovOrig > 1.0f && g_devCamFovOrig < 179.0f)) return;
    WriteFloatSafe(obj + 0x128, g_devCamFovOrig);
    Log("PatchCamera: device camera fov handed back %.3f -> %.3f\n", cur, g_devCamFovOrig);
}


int ClassifyPatchCameraOwner(void* ownerState) {
    const uintptr_t obj = reinterpret_cast<uintptr_t>(ownerState);
    if (!obj || obj < 0x10000) return 0;

    // Fast path: the overwhelming majority of calls end here.
    if (obj == g_camObjMain.load(std::memory_order_relaxed))  { ++CyberpunkVR_DebugPatchCamMain;  return 1; }
    if (obj == g_camObjVrcam.load(std::memory_order_relaxed)) { ++CyberpunkVR_DebugPatchCamVrcam; return 2; }
    if (CyberpunkVR_DeviceCamFollow && obj == g_camObjDevice.load(std::memory_order_relaxed)) {
        ++CyberpunkVR_DebugPatchCamDevice;
        StampDeviceCam();
        return 3;
    }

    const uint64_t vrcam = CyberpunkVR_VrcamCamNameHash();

    int off = g_camNameOffset.load(std::memory_order_acquire);
    if (off < 0) {
        for (int k = 0x08; k <= 0x80; k += 8) {
            uint64_t v = 0;
            if (!ReadU64Safe(obj + k, &v)) break;
            if (v == kCamNameMain || (vrcam != 0 && v == vrcam)) {
                g_camNameOffset.store(k, std::memory_order_release);
                Log("PatchCamera: component name CName found at owner+0x%02X "
                    "(main=0x%016llX vrcam=0x%016llX)\n", k,
                    static_cast<unsigned long long>(kCamNameMain),
                    static_cast<unsigned long long>(vrcam));
                off = k;
                break;
            }
        }
        if (off < 0) return 0;      // this object is not one of ours; try the next
    }

    uint64_t name = 0;
    if (!ReadU64Safe(obj + off, &name) || name == 0) return 0;
    if (name == kCamNameMain) {
        g_camObjMain.store(obj, std::memory_order_relaxed);   // latch for the fast path
        ++CyberpunkVR_DebugCamRebinds;
        ++CyberpunkVR_DebugPatchCamMain;
        return 1;
    }
    if (vrcam != 0 && name == vrcam) {
        g_camObjVrcam.store(obj, std::memory_order_relaxed);
        ++CyberpunkVR_DebugCamRebinds;
        ++CyberpunkVR_DebugPatchCamVrcam;
        return 2;
    }
    if (CyberpunkVR_DeviceCamFollow && g_remoteCamOn.load(std::memory_order_relaxed)) {
        static const uint64_t kCamNameDevice = cvr::cname_hash("cameraComponent");
        if (name == kCamNameDevice && DeviceCamPositionMatches(obj)) {
            const uintptr_t prev = g_camObjDevice.exchange(obj, std::memory_order_relaxed);
            if (prev != obj) {
                // Logged on every change of identity, and that is the diagnostic this build exists to
                // produce: ONE address while a takeover is on and nothing in between is what keying on
                // the name assumes. A stream of different addresses would mean the game patches other
                // cameras in the world too, and then the name alone is not enough.
                Log("PatchCamera: device camera component %p (was %p) hits=%u\n",
                    reinterpret_cast<void*>(obj), reinterpret_cast<void*>(prev),
                    CyberpunkVR_DebugPatchCamDevice);
            }
            ++CyberpunkVR_DebugCamRebinds;
            ++CyberpunkVR_DebugPatchCamDevice;
            g_devCamBaseValid.store(0, std::memory_order_relaxed);   // a different camera: re-latch
            g_devCamPosValid.store(0, std::memory_order_relaxed);
            StampDeviceCam();
            return 3;
        }
    }
    ++CyberpunkVR_DebugPatchCamOther;
    return 0;
}


uint64_t g_finalCameraHits = 0;





// ===================== Projection Commit Hook =====================
// Hooks the projection-data commit site. At this point
// xmm0 already contains r13[0:16] (loaded by the prior movups). The code then
// copies r13 data to the render object at rbx+0x21C0 (9 floats = 36 bytes),
// followed by xmm1 from r13[16:32], and FOV from r13[32]. We intercept to log
// the values and override the FOV.
//
// Layout at r13 (projection source, 9 floats = 36 bytes):
//   r13[0:4]   (floats 0-3): -> rbx+0x21C0 (projection params)
//   r13[4:8]   (floats 4-7): -> rbx+0x21D0 (projection params)
//   r13[8]     (float 8):    -> rbx+0x21E0 (FOV in degrees)
//
uint64_t g_unifixHits = 0;
float g_unifixProjDump[9] = {};
volatile uintptr_t g_unifixRenderObj = 0;



uint64_t g_projStageHits = 0;
float g_projStageFov = 0.0f;
float g_projStageAspect = 0.0f;
float g_projStageExtra = 0.0f;
bool g_projStagePatched = false;


// ===================== Projection FOV/Aspect Copy Hook =====================
// From ida_headless\proj4_disasm.txt and proj.txt:
//   sub_14028D4B8 @ 0x28D530: movups xmm1, [rdx+80h]
//                             movups [rcx+80h], xmm1
// The copied block contains:
//   [80h] = FOV
//   [84h] = ASPECT
//   [88h] / [8Ch] = other per-view scalars
//
// This is the first solid place where the engine copies the per-view FOV/aspect
// into the render-side struct. If aspect stays 16:9 while the VR swapchain is 1:1,
// the image stretches horizontally; here we patch the copied struct to square
// aspect directly.
//
// Strategy:
// - execute the original copy first
// - inspect src[80]/[84]
// - if it looks like a camera/projection view (FOV in a sane range, aspect ~16:9),
//   patch dst[84] = 1.0f
// - if the copied FOV is a 16:9-horizontal (>120 deg), convert it to the matching
//   square VFOV: 2*atan(tan(fov/2) * 9/16)
//

// ===================== Projection Aspect Call Hook =====================
// Real projection/aspect path from ida_headless:
//   f108294.txt
//     0x10869A: movss xmm2, [rdx+84h]
//     0x1086A2: movss xmm1, [rdx+80h]
//     0x1086AA: call sub_140109814
//
//     0x10891C: movss xmm2, [rdx+7Ch]
//     0x108921: movss xmm1, [rdx+78h]
//     0x108926: call sub_140109814
//
//     0x1089AE: movss xmm2, [rdx+84h]
//     0x1089B6: movss xmm1, [rdx+80h]
//     0x1089BE: call sub_140109814
//
// We patch the source struct that the loads read from, BEFORE the call computes the
// downstream projection. This is the first solid place in the real path where aspect
// can be made square (1.0f).

// ===================== Projection Stage Hook =====================
// render_camera_RE / ida_headless:
//   sub_14012752C @ 0x12752C  projection_from_fov_aspect
//   0x127970: movss xmm4, [rdx+80h] ; FOV
//   0x127978: movss xmm5, [rdx+84h] ; ASPECT
//   0x127980: movss xmm6, [rdx+88h]
//
// Patch only the aspect term at the exact downstream point where projection is built.




// Snap-turn yaw delta (degrees) pushed by the XInput hook when the user flicks
// the right stick. Applied here in one frame to give a true instant snap (no
// stick-driven smooth rotation). Atomic 32-bit float via bit-cast through int.
volatile LONG g_pendingSnapYawDeltaBits = 0;

// Index of the yaw float inside the delta buffer (default 1). Overridable via
// xr_snap_turn_yaw_index in vrport.ini for quick experimentation if [1] is wrong.
extern "C" int GetSnapTurnYawIndex();
// Sprint input state (left stick to the stop), written by the XInput merge each poll.
// (Kept for diagnostics; the snap-event suppression that consumed it is reverted.)






// Head-oriented locomotion: rotate the on-foot move vector by the HMD yaw so
// "forward" follows the headset. moveStruct = rsi; [+0x90]=X (strafe), [+0x94]=Y
// (forward). Only active in HMD movement mode and outside menus; the vehicle path
// never hits OnFootMoveXY so driving is untouched.





// ===========================================================================

// Redirect every "XInputGetState" import slot in a module's IAT to newFunc.
// Unlike an inline entry-point patch this never rewrites the bytes of the
// (Windows-version-specific) XInput DLL, so it cannot corrupt a relative
// instruction and crash on a machine whose XInput1_4.dll differs from the
// dev's -- the exact failure that "xr_xinput_install=1" caused on some setups.
// It also composes with anything that already hooked the slot (e.g. Steam
// Input): the previous slot value is chained back as the "real" function.

// Boots the stereo module (sync_stereo). Defined further down inside the extern "C" block that
// wraps the DXGI exports, hence the matching linkage here; declared this early because
// WorkerThread must run it before it claims the node dispatcher.
// InitStereoOnce is declared in Core/CoreInternal.hpp. It used to be forward-declared here as
// `extern "C" { static ... }`, which is both internal linkage and C linkage -- neither survives
// another translation unit calling it, and WorkerThread.cpp does.

// Moved to src/Core/WorkerThread.cpp: the background thread that does all of the polling.

extern "C" {
// Initialize OpenXR early
void InitOpenXREarly() {
    static thread_local bool s_initOpenXRReentry = false;
    if (s_initOpenXRReentry) {
        return;
    }
    s_initOpenXRReentry = true;
    OpenXRManager::Get().Init();
    s_initOpenXRReentry = false;
}

// Enable DRED auto-breadcrumbs + page-fault reporting before any D3D12 device
// is created. Implemented in swapchain_hooks.cpp.
extern "C" void CyberpunkVRPort_EnableDredOnce();

// ---- sync_stereo boot ---------------------------------------------------------------------
// extern "C++" is load-bearing: this sits inside the extern "C" block that wraps the DXGI
// exports, and without it these would be declared with C linkage and never find the C++
// definitions in sync_stereo.cpp.
extern "C++" {
namespace cvr {
void sync_stereo_init();
void sync_stereo_install_early_hooks();
}
}
// Live kill switch, exported so it can be flipped from the debugger, and a file escape hatch
// for a bad boot: dropping bin\x64\vrport_nostereo.txt keeps the engine hooks out entirely
// without a rebuild. Stereo is the default now, so the file is an opt-OUT (the old build had
// the opposite, vrport_stereo.txt, back when the module was the experiment rather than the
// shipping path).
extern "C" __declspec(dllexport) int CyberpunkVR_StereoModuleEnable = 1;
extern "C" __declspec(dllexport) int CyberpunkVR_StereoModuleLoaded = 0;


void InitStereoOnce() {
    static bool s_done = false;
    if (s_done) return;
    s_done = true;

    if (!CyberpunkVR_StereoModuleEnable) {
        Log("Stereo: module disabled by CyberpunkVR_StereoModuleEnable=0\n");
        return;
    }
    char optOut[MAX_PATH];
    GetModuleFileNameA(nullptr, optOut, MAX_PATH);
    if (char* slash = strrchr(optOut, '\\')) {
        *(slash + 1) = 0;
        strcat_s(optOut, "vrport_nostereo.txt");
        if (GetFileAttributesA(optOut) != INVALID_FILE_ATTRIBUTES) {
            Log("Stereo: vrport_nostereo.txt present -- engine hooks not installed\n");
            return;
        }
    }

    // Must run BEFORE the game's D3D12CreateDevice: the descriptor-heap probe enlarges the
    // shader-visible CBV_SRV_UAV heap the second view needs, and that size is fixed at device
    // creation. This is why it boots here and not from WorkerThread, which sleeps 8 s first --
    // by then the device is long since created. Same guarantee DRED relies on above.
    // Before a single hook is installed, so no probe has had a chance to fire yet.
    ApplyLauncherDebugGate();
    cvr::sync_stereo_init();
    cvr::sync_stereo_install_early_hooks();
    CyberpunkVR_StereoModuleLoaded = 1;
    Log("Stereo: sync_stereo engine hooks installed\n");
}

// Entry point for the RED4ext plugin. As a proxy this was driven from the DXGI factory
// exports below; a plugin has no such call, so it boots the stereo module directly.
__declspec(dllexport) void CyberpunkVRPort_InitStereo() { InitStereoOnce(); }

}


