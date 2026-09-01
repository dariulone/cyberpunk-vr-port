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
#include "Camera/CameraState.hpp"   // PatchFastPath, PatchFastDisarm
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
    if (v != s_lastLogged) { s_lastLogged = v; if (g_verboseLog) Log("VRIK bridge: disable_mouse_y=%d -> %s\n", v, g_vrikSettingsPath); }
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

// The fast-path block the PatchCamera trampoline reads. Defined here, beside the latches it
// mirrors, so there is exactly one owner of both. See CameraState.hpp for why it is armed the way
// it is.
// THE SECOND EYE'S CAMERA IS ONLY REBUILT WHEN A DIRTY FLAG SAYS SO, AND NOTHING EVER SET IT.
//
// Read out of the engine, sub_140AC316C -- the driver that owns the RTT camera each tick:
//
//     140AC3183  lock cmpxchg [rdi+8E0h], ecx   ; rdi = comp+0x120, so the flag is comp+0xA00
//     140AC3192  test    eax, eax               ; eax = the value that was there
//     140AC3194  jz      loc_140AC31A8          ; it was 0 -> SKIP the build
//     140AC31A3  call    sub_140AC2BA4          ; builds the view from comp+0xE0 and comp+0xF0
//     140AC31BD  jmp     sub_1404FBAFC          ; the view is CREATED either way
//
// So the transform we write into the component is consumed only on ticks where something has marked
// the camera dirty. The creation runs regardless, which is exactly the reported symptom: "анимации и
// т.д все нормальное, лагает только движение камерой" -- a fresh image every frame, built from a
// camera nobody refreshed.
//
// In ordinary play, and in a braindance that swaps the player for the replacer, the eye's parent moves
// and the engine marks it dirty on its own. In a braindance that does NOT swap the player, the body
// stands still for the whole replay and the flag is never raised: measured, sub_140AC2BA4 was reached
// 9.1 times a second in a braindance that looks right and ZERO times in the one that does not, with a
// hardware breakpoint on the same address in the same session. Neither this port nor upstream has ever
// set that flag, which is why the defect is in 0.1.5, in this branch and in upstream alike.
//
// Bit 1 = raise the flag, bit 2 = write a fresh pose in first. Both, because the flag alone rebuilds
// per frame from a pose that only PatchCamera refreshes -- and in that braindance PatchCamera reaches
// the component 3.4 times a second (measured: +46 over 13.55 s).
//
// THE HAZARD, stated because this project has already paid it: an older note by
// CyberpunkVR_VrcamFovDeg records that "forcing comp+0xA00 or calling sub_140AC316C drags view-create
// in, which hitched the game and hung the GPU". That was forcing it from the wrong place. Here it is
// one store per view-create, on the thread that owns the driver, and only while a braindance is
// running -- and it is a live key so it can be taken away without a build.
// DEFAULT 0, AND IT REMOVED THE DEVICE. Raising comp+0xA00 by hand once per view-create hung the
// GPU inside a minute -- DXGI_ERROR_DEVICE_HUNG 0x887A0006, frame rate collapsing first -- because
// it forces the view rebuild while bypassing every piece of bookkeeping the engine does around it.
// The flag is a RESULT of the engine's transform notify, not an input. Kept switchable only so the
// experiment is on record; CyberpunkVR_BdPushTransform is the route that goes through the notify.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_BdCamDirty = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugBdCamDirty = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugBdCamPose = 0;

// THE SECOND EYE'S TRANSFORM, PUSHED THROUGH THE ENGINE'S OWN NOTIFY -- AND WHY THE RAW FLAG WAS
// THE WRONG ANSWER.
//
// The component's transform setter, sub_1401D8558, is where this port's PatchCamera hook already
// lives (exe+0x1D8A8F is its `movups [rdx], xmm0`, the quaternion store). Read out around it:
//
//     1401D8A73  movsd   [rsi+0E0h], xmm0      ; position
//     1401D8A8F  movups  [rdx], xmm0           ; quaternion, rdx = rsi+0xF0   <- our hook
//     1401D8A92  cmp     byte [rsi+0B1h], 0    ; a "force the notify" request byte
//     1401D8AAF  cmp     r9d, [rsi+0E0h]       ; ...otherwise: did the transform actually change?
//     1401D8AD6  call    sub_1401D9958         ; quaternion compare, with an epsilon
//     1401D8AF1  call    qword ptr [rax+240h]  ; CHANGED -> the engine's own "transform updated"
//     1401D8AF7  mov     byte [rsi+0B1h], 0
//
// That virtual is what eventually raises the RTT camera's rebuild flag at comp+0xA00, and forcing
// that flag directly instead -- which is what the first attempt did, once per view-create on the
// render thread -- hung the GPU inside a minute: DXGI_ERROR_DEVICE_HUNG, 0x887A0006, with the frame
// rate collapsing first. Bypassing an engine notification to poke the state it maintains is how that
// is earned; the older note by CyberpunkVR_VrcamFovDeg had already said as much.
//
// So the transform goes in the way the engine puts it in: write the fields, then call the component's
// own vtable+0x240 with the arguments the setter uses (rcx = component, rdx = component+0x100).
// From LocateCamera, because that runs once per frame ON THE GAME THREAD -- the thread the setter
// itself runs on -- and because the pose it composes is the one MAIN is about to render from, so both
// eyes come from one composition.
//
// WHY IT IS NEEDED AT ALL: the setter is only called when something moves the component's parent. In
// ordinary play, and in a braindance that swaps the player for the replacer, that is every frame. In
// a braindance that does NOT swap the player, the body stands still for the whole replay and the
// setter was measured at 3.4 calls a second against 70+ rendered frames -- for BOTH cameras, which is
// why the first eye is affected too.
//
// BIT 1 = the second eye. Confirmed on the picture: VRCAM stopped lagging the moment it was armed,
// which also answers the obvious objection -- VRCAM is normally written at PatchCamera, but
// PatchCamera IS this setter, and in that braindance the setter is not called. This does the
// setter's work instead, every frame, so the second eye stops depending on it.
// BIT 2 = MAIN's own camera component, the same way.
//
// MAIN needs it for the same reason, and it is worth saying which half of MAIN's problem it fixes.
// The replay is rendered through the located buffer, whose position the engine fills from MAIN's
// camera component -- the same setter, the same standing body, the same 3.4 calls a second. So
// MAIN's VIEWPOINT is stale exactly as the second eye's was. Its ROTATION is a separate question:
// with CyberpunkVR_BdQuatFromWriteSite on, that comes from whatever PatchCamera last composed, which
// is also 3.4 times a second -- pushing it through the notify makes it arrive reliably, not sooner.
// Both keys stay live so the two can be told apart in one session.
//
// Bit 2 is OFF by default: MAIN's component is the FPP camera the body, the arms and the weapon all
// hang off, so a write there reaches more than the picture.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_BdPushTransform = 1;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugBdPushTransform = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugBdPushMain = 0;


// BOTH EYES FROM ONE COMPOSITION AT ONE RATE -- the missing quarter of it being MAIN's viewpoint.
//
// In a braindance the four quantities that place the stereo pair came from three different cadences,
// and every knob tried so far only moved which one lagged:
//
//     MAIN   rotation   the frame's own composition        per frame
//     MAIN   position   the engine, from the component     at the setter's rate
//     VRCAM  rotation   the same composition, pushed       per frame
//     VRCAM  position   the scene pose, pushed             per frame
//
// One of the four at a different rate is not "a bit of lag in one eye": it is the pair coming apart,
// and it shows in EVERY braindance, including the one that otherwise looks right -- which is what the
// user pointed out and what the rate-based auto-switch I started writing would have papered over.
//
// This closes it: MAIN's viewpoint is written into the located buffer from the same per-frame scene
// pose the second eye is placed with, so all four move together. The half-IPD is NOT applied here --
// CyberpunkVR_BdIpdInLocate already adds MAIN's half further down, and the push adds the second eye's
// with the opposite sign, so the pair stays symmetric about this base.
//
// No engine notify is needed and none is made: nothing else consumes this buffer, and it is the
// descriptor the replay is rendered through.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_BdMainPosFromScene = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugBdMainPos = 0;

// WHAT THE SECOND EYE IS PLACED RELATIVE TO.
//   0  the scene pose the CET script publishes
//   1  MAIN's own located position for THIS frame
//
// 1, and the reason 0 was ever needed is gone. Two things named it:
//
//   * "в просмотре редкие дерги у обоих глаз" -- both eyes at once means the shared source is
//     stepping, and the only shared source was the script's scene pose. CET's update is not strictly
//     per frame, so it steps occasionally; placing the pair relative to MAIN's own position takes the
//     script out of the pair's geometry entirely.
//   * "в редактировании VRCAM выше чем main" -- in the editor the scene owns nothing, the scene pose
//     is invalid, the push stood down and the eyes fell back to their own bases: MAIN's located
//     camera against the second eye's attachment point on the body, which sits at a different height.
//
// And "pos from main" was already tried and rejected as lagging -- correctly, because it was consumed
// at PatchCamera, which runs BEFORE LocateCamera publishes it, so it was always a frame old. The push
// lives INSIDE LocateCamera, where this frame's own value is in hand, so the same idea carries no lag
// at all. That is the whole difference between the two attempts.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_BdPushBase = 1;

// ONE COMPOSITION AND ONE WRITER FOR THE PAIR, WHILE THE BRAINDANCE PUSH OWNS THE SECOND EYE.
//
// Two separate desyncs, both from the same shape -- the same quantity produced twice.
//
// THE SAMPLE. With compose-at-write on, PatchCamera samples the head AGAIN at the write site, so the
// second eye is built from this site's sample while MAIN is built from the one LocateCamera composed.
// Two head samples for one stereo pair is a skew between the eyes however small the gap, and
// CyberpunkVR_BdQuatFromWriteSite was an attempt at the same problem from the other end: it made MAIN
// adopt THIS value. That is right only where this site runs every frame -- in the braindance without
// the replacer it runs 3.4 times a second, so adopting it pinned MAIN to that rate, reported as MAIN
// lagging. Standing the local composition down instead leaves exactly one composition, taken once per
// frame in LocateCamera, and both eyes read it.
//
// THE POSITION. The push writes the second eye's full viewpoint -- MAIN's centre plus half an IPD.
// PatchCamera then READS that back, adds the head displacement and half an IPD again, and stores it:
// the same offsets counted twice, alternating with the push's own value. That is the left eye
// juddering in the editor while MAIN was clean, reported exactly that way.
//
// Both are suppressed only while the push is armed AND a braindance is running, so nothing outside
// that window changes, and 0 restores the old behaviour whole.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_BdOneComposition = 1;

// One predicate for both sites, because the composition and the FRAME POSE LABEL have to move
// together. Whoever composes what the pixels are drawn from must also be the one that publishes the
// pose the frame is labelled with -- the note beside PushRenderHeadPose in LocateCamera says why:
// publishing from two sites lets whichever ran last label the image with a pose that was never
// written into the camera, and that mismatch is what the compositor turns into judder. It is not
// visible on the flat monitor, which has no compositor in the way.
// AND ONLY WHILE THE SCENE OWNS THE CAMERA -- i.e. in playback, not in the editor.
//
// The push exists to substitute for a transform setter that is not being called: during a replay the
// body the camera components hang off stands still, so the engine never updates them. In the EDITOR
// the player flies the camera themselves, the setter runs every frame as usual, and the push becomes
// a second writer for a quantity that already has one -- the two values alternate and both eyes
// judder, since by then they share one composition. Reported exactly that way: playback clean, editor
// juddering in both eyes.
//
// The port's own CET script already publishes this distinction as the validity of the scene pose
// (GetSceneSystemCameraControlEnabled), so nothing new has to detect it.
bool BdPushOwnsComposition() {
    if (CyberpunkVR_BdOneComposition == 0 || (CyberpunkVR_BdPushTransform & 1) == 0) return false;
    // A TAKEOVER DELIBERATELY DOES NOT OWN IT. It was tried: this predicate also suppresses
    // PushRenderHeadPose and the ring entry in PatchCamera, on the grounds that LocateCamera publishes
    // them instead -- which it only does inside the braindance branch. During a takeover that left the
    // frame with no pose label at all, and an unlabelled frame is what the compositor reprojects wrongly.
    // The takeover keeps PatchCamera's composition and label, and only the WRITES move.
    return g_bdActive.load(std::memory_order_relaxed) &&
           g_bdScenePoseValid.load(std::memory_order_acquire);
}

// "The located buffer owns this frame's camera": a braindance scene, or a device takeover once the lens
// has actually been identified. Both mean the same thing to every site that asks -- the base comes from
// the buffer and the second eye is written from here, not from its component.
// "Did the classifier ever claim the taken-over camera": the latch itself lives further down the file.
static bool DeviceCamClaimed();

bool LocateOwnsTakeover() {
    if (CyberpunkVR_DevCamInLocate == 0) return false;
    if (!DeviceCamActive() || !g_devCamBaseValid.load(std::memory_order_acquire)) return false;
    // ...AND ONLY WHERE THERE IS NO DISPATCH OF ITS OWN TO WRITE.
    //
    // A surveillance camera on a wall DOES reach the transform hook: it is claimed as kind 3 and the port
    // writes the composition into that camera's own camera state, which is the mechanism that has worked
    // all along and must not be replaced by this one. The AV turret never arrives there -- measured, not
    // one claim in a session, with a hardware write breakpoint on the lens stamp that never fired -- so it
    // has no dispatch of its own and the located buffer is the only place left to write.
    //
    // The claim is the discriminator, so each case picks its route from a fact rather than from a key:
    // claimed means "drive it where the engine hands it to us", unclaimed means "drive the buffer".
    return !DeviceCamClaimed();
}

// THE BRAINDANCE EDITOR'S HEIGHT MISMATCH.
//
// While a braindance runs but the scene does NOT own the camera -- the editor, where the player flies
// the camera themselves -- the two eyes fall back to their own bases: MAIN to the camera the engine
// locates, the second eye to its attachment on the player's body, which sits at a different height.
// Reported as "в редактировании VRCAM выше чем main по позиции", and it came back the moment the
// braindance push was correctly confined to playback.
//
// WHICH base, and the first answer was wrong. MAIN's COMPONENT position -- CyberpunkVR_EngineCamPosFP
// -- is the camera on the player's BODY, and in the editor the body stands still while the player
// flies a free camera: MAIN renders from the located buffer, not from that component. Placing the
// second eye on the body therefore put it somewhere the world was not prepared for it, and the left
// eye flashed black every few frames.
//
// The located centre is the place MAIN actually renders from, so that is the base. It is published by
// LocateCamera, which runs AFTER this site, so it is one frame old -- and that is the right trade
// here: a frame of lag on a camera the player flies by hand is invisible, while the wrong PLACE is
// not. In playback this branch does not run at all; there the base is the scene pose.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_BdEditorAlign = 1;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugBdEditorAlign = 0;

// THE SCENE'S ROTATION, TAKEN FROM THE ENGINE RATHER THAN FROM LUA.
//
// The head is composed onto the scene camera's own rotation while a replay plays, and that base came
// from g_bdSceneQuat -- published by the CET script, whose update is not per frame. So the base
// stepped while the head moved smoothly, and the pair juddered exactly when the engine was driving
// the scene: "когда движок начинает вести сцену и получается рассинхрон позы появляются дерги".
// Position had already been taken off the script; this is the other half.
//
// The engine's own value is in hand and costs nothing: the located buffer is filled from the camera
// system BEFORE this callback runs, so the quaternion at entry is the scene camera's rotation for this
// frame. It is captured before a single field is written, which is the only ordering that matters --
// this hook writes that same quaternion further down, and reading it back afterwards would be reading
// our own output. The script's value stays as the fallback for the frames before the first locate and
// for anything the plausibility test rejects.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_BdBaseFromLocate = 1;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugBdBaseFromLocate = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugBdBaseFromScript = 0;

PatchFastPath g_patchFast{};

// The two halves of the re-arm condition. Plain statics on purpose: they are written on the slow
// path and must NOT sit in g_patchFast, whose line has to stay read-shared.
static bool g_pfSeenMain = false;
static bool g_pfSeenVrcam = false;

void PatchFastDisarm() {
    g_pfSeenMain = false;
    g_pfSeenVrcam = false;
    if (g_patchFast.armed != 0u) g_patchFast.armed = 0u;   // no store in the steady state
}
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

// WHERE THE SECOND EYE TAKES ITS POSITION FROM.
//   0  its own attachment, plus whatever the branches in PatchCamera hand it (current behaviour)
//   1  MAIN's base in a braindance, but ONLY where the scene does not own the camera -- that is
//      the editor, where the player flies the camera. In playback the scene pose wins and this
//      mode changes nothing: reported from the picture, playback went wrong the moment MAIN's
//      base was preferred there.
//   2  MAIN's base wherever there is no scene pose, braindance or not
//   3  the second view sits on MAIN's HEAD CENTRE as located, and has no head translation of
//      its own at all. Whatever MAIN carries the second eye carries, because it is the same
//      number -- no per-view delta to switch on in one mode and off in another.
//
// Mode 1 exists because of a measurement rather than a theory. In a braindance the second eye takes
// the pose the SCRIPT publishes for the scene camera (g_bdScenePosFP, written from RemoteCamera),
// and the field log says that is not where MAIN renders from:
//
//     mainPos  = (-1720.973, -1235.205, 23.665)
//     vrcamPos = (-1722.116, -1236.144, 23.544)
//
// 1.48 m apart horizontally and 12 cm vertically, where the residual after the head delta should be
// one IPD. MAIN's base needs no identification and no pose hunt: it is read from MAIN's own
// component at +0xE0 before anything of ours moves it, which is by definition where the first eye
// renders. Default 0 until it has been seen on the picture.
// DEFAULT 0, AND MODE 3 IS NOT THE ANSWER -- the paragraph above is the older claim and it did not
// survive the picture. Mode 3 takes the second view's base from the located centre, and that base is a
// TICK OLD relative to what MAIN renders from, so the second eye trails the first; the user has reported
// this twice ("3 ключ ... дает отставание от main"), and the braindance path deliberately takes its base
// from elsewhere for the same reason. Retried once anyway while chasing a 5.5 m divergence in an AV and
// it changed nothing there, because that divergence had another cause entirely (a turret published as a
// taken-over surveillance camera). Leave this at 0 unless a measurement says otherwise.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_VrcamPosFromMain = 0;
// THE VIEWPOINT OF A TAKEN-OVER CAMERA, in metres, in the LENS's own frame: x right, y forward, z up.
//
// Asked for as a slider, and asked for because the point is wrong rather than unknown: manning the AV's
// turret in the Silverhand mission puts the view at the turret's own camera, which is not where a person
// sitting there would have their eyes ("не с той точки я смотрю"). No amount of identification fixes
// that -- it is a number only the person looking can choose.
//
// YAW-ONLY FRAME, deliberately. The mount is nose-down (measured at 9.4 degrees elsewhere in this file),
// so composing the full lens orientation would send "forward" into the ground and "up" through the
// fuselage. Forward is the aim's horizontal direction, up is world up, which is what a slider labelled
// up/down has to mean.
//
// Applied to BOTH eyes, after the lens position is established and before the IPD split, so it can never
// become a disparity between them -- the same shape the IPD half itself uses, which is why re-applying it
// every pass is safe here (the engine rewrites +0xE0 between passes; if it did not, the IPD would spiral
// too and this port would have noticed long ago).
// A TAKEN-OVER CAMERA GOES THROUGH THE LOCATED BUFFER, the same route a braindance scene uses.
//
// Measured at the AV turret, and by an A/B the user flipped in the panel: with the scene path open MAIN's
// orientation tracked the head again and the second eye stopped jerking; with it closed both stopped. The
// reason is in this file's own machinery -- the located buffer takes its base from the BUFFER the engine
// is filling this frame (CyberpunkVR_BdBaseFromLocate), so it does not depend on which object the
// classifier has latched. And the latch cannot be trusted here: in that mission four objects answer to
// the component name `camera`, the latch changed 696 times in six seconds, and the census had MAIN 3.8 km
// from the second eye with a 1.7 m "head delta".
//
// What it does NOT reuse is the scene POSE: during a takeover the scene camera is somewhere else
// entirely, and taking MAIN's position from it is what made MAIN teleport for a frame. The position stays
// the buffer's own -- the lens the engine is rendering through -- and only the eye split is added.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_DevCamInLocate = 1;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugTakeoverPush = 0;

// THE NAME IS NOT THE IDENTITY -- the header of VRRemoteCamera says so in as many words, and then the
// claim below tested the name anyway. It cost this: the AV turret's camera component is NOT called
// `cameraComponent`, so on first entering the turret nothing was ever claimed -- no lens, no fov write,
// no push -- and the picture stayed at the mount's own 60 degrees. Toggling the scene path on and off
// "fixed" it only because the fov-match claim runs without the name test and left the address latched.
// Reported exactly that way: "когда ты только попадаешь в турель fov не меняется. если переключить на
// scene A/B потом выключить то вроде ок".
//
// 1 = trust the published position alone, which is what the script publishes it for. 0 = the old
// name-and-position test, kept because this is the identification path and a wrong claim here moves the
// picture.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_DevCamAnyName = 1;

extern "C" __declspec(dllexport) float CyberpunkVR_DevCamOffsetX = 0.0f;
extern "C" __declspec(dllexport) float CyberpunkVR_DevCamOffsetY = 0.0f;
extern "C" __declspec(dllexport) float CyberpunkVR_DevCamOffsetZ = 0.0f;


// THE FIRST EYE'S HALF-IPD IN A BRAINDANCE, APPLIED IN LocateCamera INSTEAD OF PatchCamera.
//
// In ordinary play both eyes take their separation at the component: PatchCamera writes -half into
// MAIN's and +half into VRCAM's, and the located buffer is filled from the component afterwards, so
// it arrives already carrying it. A braindance has no camera component for MAIN at all -- the replay
// drives a virtual camera the port cannot reach, which is why MAIN's ORIENTATION is already written
// from LocateCamera there. The position half was still going to the FPP component, which renders
// nothing in that mode, so it reached no picture.
//
// Measured in braindance playback, current build:
//     scene camera (live, through the CET bridge)   -1722.111 -1236.101 23.551
//     VRCAM as patched                              -1722.112 -1236.133 23.551
//     ipd 0.0640, half 0.0320 -- and the gap between them is 0.032
// So the second eye carried its half and the first carried none: the eyes were separated by HALF an
// IPD, which reads as everything being twice as far away. 1 puts MAIN's half into the located buffer,
// which is the descriptor the engine renders the braindance through (measured 0.9995 against it).
// Only while the scene owns the camera -- in the editor the component is live again and PatchCamera
// is the right site, exactly as in ordinary play.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_BdIpdInLocate = 1;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugBdIpdLocate = 0;

// MAIN'S POSITION AS THE ENGINE WILL RENDER IT, published from LocateCamera after everything of
// ours has gone into that buffer. It exists for the camera census: in a braindance MAIN's COMPONENT
// is not what the picture comes from, so a census that differences the two components reports the
// gap between the player's body and the replay camera -- 1.737 m -- and says nothing at all about
// the eye separation it is there to check.
std::atomic<int32_t> g_locatePosFP[3] = {};
std::atomic<int32_t> g_locatePosValid{0};

// MAIN's HEAD CENTRE as located: the same buffer, taken BEFORE the eye separation goes in. It is
// the base both eyes belong on, and it already carries whatever translation MAIN has -- the
// room-scale delta in ordinary play, the replay's own movement in a braindance, nothing at all
// where the game gives nothing. That is the point: the second view should not compute a
// displacement of its own and then have it switched off case by case, it should sit on MAIN's.
std::atomic<int32_t> g_locateCenterFP[3] = {};

// THE BASE THE HEAD IS COMPOSED ONTO IN A BRAINDANCE.
//   0  the pose the CET script publishes for the scene camera (g_bdSceneQuat)
//   1  the located buffer's OWN incoming quaternion -- the engine's value for the camera being
//      located, read this frame, before anything of ours goes in
//
// The script route is a tick old by construction: it is published from a Lua update and the render
// outruns it, which this codebase already noticed from the other end ("the published pose is a tick
// old and this pan outran it"). A fresh head composed onto a stale base lags and then catches up --
// judder on head turns, in BOTH eyes, because both are composed from the same value. The buffer's
// own quaternion is the same camera one frame fresher and costs nothing to read.
// THE SCENE'S OWN FOV, published by the script that already reads it every tick.
//
// The braindance fov write identifies its view two ways: by the scene pose, and -- far more often --
// by matching the view's fov against one LEARNED from an earlier pose match. Measured in playback:
// 1747 of 1801 matches came through the learned-fov path and only 54 through the pose. That makes
// the pose test a BOOTSTRAP, and until it lands once nothing matches at all: the first seconds of a
// playback render at the scene's own fov, and every new scene starts over because its fov is
// different from the one learned in the last. Reported exactly that way -- "at first appearance the
// FOV is not applied, and coming back from editing it is not applied immediately".
//
// Seeded from the script instead, the learned value is right from the first frame and no pose match
// is needed to start. The script has had this number all along (Game.GetCameraSystem():
// GetActiveCameraFOV) and was only using it for its own status line.
std::atomic<float> g_bdSceneFov{0.0f};

// THE SCENE'S QUATERNION AS THE COMPOSE BASE AT THE WRITE SITE TOO, so both eyes are composed
// onto the same thing in a braindance. Without it the second eye sat on the player's body
// heading, which a braindance never turns, while MAIN followed the replay -- two bases, one
// stereo pair. 0 restores the split.
// 1 = in a braindance MAIN writes the quaternion the WRITE SITE composed, so the pixels and the
// submitted pose label come from one head sample. 0 = this site composes its own, which is what
// left MAIN juddering in the headset while the second eye, written at the write site, did not.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_BdQuatFromWriteSite = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugBdQuatFromWriteSite = 0;
extern "C" __declspec(dllexport) int32_t CyberpunkVR_BdSceneBaseInPatch = 1;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugBdSceneBaseInPatch = 0;

// DISPROVEN AND OFF, kept only so the experiment is not repeated. Basing the braindance head compose
// on the located buffer's own quaternion looked strictly fresher than the script pose -- and it is
// the same array this hook WRITES further down, so when the engine has not refilled it the base is
// our own previous output and the head is composed onto itself. Reported as judder that got worse.
// The warning was already in this codebase: reading our own output back as a base is what once made
// the camera spin up without bound.
//   0  the pose the script publishes (a tick old, but never fed back)
//   1  DISPROVEN, do not use: the buffer taken unconditionally, which reads our own output back
//   2  the buffer, but only on a call where it does NOT hold what we last published
// DEFAULT 0, AND BOTH NON-ZERO MODES ARE DISPROVEN ON THE PICTURE. The judder they were written
// for was never the age of the base: it was the two eyes being composed onto DIFFERENT bases,
// and once the write site was given the scene's own quaternion the base here has to stay the
// same one or the split comes back. Kept only so the experiment is not repeated.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_BdQuatFromBuffer = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugBdQuatFromBuffer = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugVrcamPosFromMain = 0;
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
    if (g_verboseLog) Log("[VR] VehicleComponent::IsDriver %s -- wheel grab is %s\n",
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
// WHICH ENTITY THE PLAYER TOOK OVER, as an identity rather than as a place.
//
// Position was the identity until now, and on a static surveillance camera it is a good one. On the AV
// turret it is not one at all: the platform flies, so a published point is metres stale by the time the
// classifier tests it (measured: published (-1450.9, 177.8, 623.5) against a lens at (-1437.3, 197.9,
// 623.9)), and the AV carries several camera components inside any tolerance wide enough to survive that
// -- 154643 latch changes in one session. The result was no claim at all: DebugPatchCamDevice = 0, and
// with it no fov, no lens and the second eye left on the player's camera.
//
// The script knows the object exactly, so it publishes its EntityID and the plugin walks the two hops the
// SDK documents: component+0x50 is the owner Entity (RED4ext ent::IComponent::owner), entity+0x48 is its
// EntityID (ent::Entity::entityID). The name CName the classifier already reads sits at component+0x40 in
// that same layout, and the live log confirms the calibration found it exactly there -- so the base is the
// component and these offsets apply.
//
// Zero means "not published": the position test then stands in, which is the surveillance camera's case
// and stays untouched.
std::atomic<uint64_t> g_takeoverEntityId{0};

// THE LENS COMPONENT ITSELF, resolved once and then read fresh every frame.
//
// This exists because the mount's camera NEVER reaches the port's camera hook -- measured with a hardware
// write breakpoint on g_devCamPosFP that did not fire once in five seconds in the turret, with
// g_remoteCamOn = 1, no claim logged and no rejected candidate either. There is nothing to identify in
// that callback because the object never arrives, so no identification scheme could have worked there.
//
// What the script CAN do is hand the object over. CET's userdata for a game object holds a handle to it,
// so the address of that userdata plus a VERIFICATION yields the pointer without guessing: a candidate is
// accepted only if the component name CName at +0x40 is `cameraComponent` AND its owner entity's id at
// owner+0x48 equals the id the script published for the object it took over. Both facts are read out of
// the object itself, so a wrong pointer cannot pass both.
//
// Why the pointer and not a published pose: a pose from a script tick is up to a frame old, and on a
// platform moving at tens of metres per second one frame is metres -- the two eyes would sit apart along
// the flight path, which is the lag that was reported earlier. Reading the component at the moment of the
// write has no such term.
std::atomic<uintptr_t> g_lensComp{0};

// READABLE, ASKED OF THE OS RATHER THAN ASSUMED. __try/__except does not catch a bad read here -- the
// engine's vectored handler takes the exception first -- and this scan learned that the hard way: a qword
// holding the float 1.0 (0x3F800000) was dereferenced and killed the process at ReadPtrSafe. VirtualQuery
// costs real time, so it is only ever called while BINDING, which happens once per takeover.
static bool AddressIsReadable(uintptr_t a, size_t bytes) {
    if (a < 0x10000 || a >= 0x7FF000000000ull) return false;
    if ((a & 7u) != 0) return false;                       // every object here is qword aligned
    MEMORY_BASIC_INFORMATION mbi = {};
    if (VirtualQuery(reinterpret_cast<void*>(a), &mbi, sizeof(mbi)) != sizeof(mbi)) return false;
    if (mbi.State != MEM_COMMIT) return false;
    const DWORD noRead = PAGE_NOACCESS | PAGE_GUARD;
    if (mbi.Protect & noRead) return false;
    const uintptr_t end = reinterpret_cast<uintptr_t>(mbi.BaseAddress) + mbi.RegionSize;
    return (a + bytes) <= end;
}

// THE ENTITY WHOSE CAMERA IS BEING LOOKED THROUGH, kept so the lens can be looked up inside it.
std::atomic<uintptr_t> g_takeoverEntity{0};

// THE LENS, FOUND IN THAT ENTITY'S OWN COMPONENT LIST.
//
// This replaces a scan of CET's userdata, which did not work and cost a crash: the instance pointer is
// not in the first qwords of that userdata, and a qword holding the float 1.0 got dereferenced
// (0x3F800040). Measured instead, in the debugger, while sitting at the turret: components of the
// taken-over entity DO reach the camera hook -- one of them was claimed -- so the ENTITY pointer is
// available, and ent::Entity carries its components at +0xA0 in a layout the SDK states exactly:
//
//     entity + 0xA0   T*      entries    DynArray<Handle<IComponent>>; Handle is 0x10, instance first
//     entity + 0xA8   uint32  capacity
//     entity + 0xAC   uint32  size
//
// Read off this AV: capacity 87, size 59, and the entries are (instance, refcount) pointer pairs exactly
// as Handle<IComponent> describes. So the lens is "the component named `cameraComponent` whose owner is
// the entity the script named", and both halves come out of the objects themselves -- no position, no
// fov, no tolerance, nothing a flying platform or a sibling camera can break.
uintptr_t BindLensFromEntity(uintptr_t entity) {
    static const uint64_t kCamComp = cvr::cname_hash("cameraComponent");
    if (!AddressIsReadable(entity, 0xB0)) return 0;
    uint64_t entries = 0;
    uint32_t cap = 0, count = 0;
    if (!ReadU64Safe(entity + 0xA0, &entries)) return 0;
    if (!ReadU32Safe(entity + 0xA8, &cap)) return 0;
    if (!ReadU32Safe(entity + 0xAC, &count)) return 0;
    if (count > cap) count = cap;          // a size past the capacity is a bad read, not a long list
    if (count > 512u) count = 512u;
    const uintptr_t arr = static_cast<uintptr_t>(entries);
    for (uint32_t i = 0; i < count; ++i) {
        const uintptr_t slot = arr + static_cast<uintptr_t>(i) * 0x10;
        if (!AddressIsReadable(slot, 0x10)) continue;
        uint64_t inst = 0;
        if (!ReadU64Safe(slot, &inst)) continue;
        const uintptr_t c = static_cast<uintptr_t>(inst);
        // Readable as far as the rotation, because that is what the per-frame refresh takes from it.
        if (!AddressIsReadable(c, 0x100)) continue;
        uint64_t nm = 0;
        if (!ReadU64Safe(c + 0x40, &nm) || nm != kCamComp) continue;
        uint64_t owner = 0;
        if (!ReadU64Safe(c + 0x50, &owner) || static_cast<uintptr_t>(owner) != entity) continue;
        g_lensComp.store(c, std::memory_order_release);
        int32_t p[3] = {};
        for (int k = 0; k < 3; ++k) {
            uint32_t v = 0;
            if (ReadU32Safe(c + 0xE0 + k * 4, &v)) p[k] = static_cast<int32_t>(v);
        }
        const float kfp = 1.0f / 131072.0f;
        if (g_verboseLog) Log("PatchCamera: lens is component %p of entity %p, at (%.3f, %.3f, %.3f), %u component(s)\n",
            reinterpret_cast<void*>(c), reinterpret_cast<void*>(entity),
            static_cast<double>(p[0] * kfp), static_cast<double>(p[1] * kfp),
            static_cast<double>(p[2] * kfp), count);
        return c;
    }
    if (g_verboseLog) Log("PatchCamera: entity %p carries no cameraComponent among %u component(s)\n",
        reinterpret_cast<void*>(entity), count);
    return 0;
}

// Fills exactly the globals the existing device-camera path already consumes, so nothing downstream
// changes: the second eye copies this position and composes onto this base, which is code that has
// already been through several rounds of measurement.
void StampDeviceCam();   // defined below; the fresh read is what keeps the device path alive now

// WHAT WE LAST WROTE INTO THE LENS. Kept so the component's own aim can be told from our composition
// coming back through the same field -- see the note in RefreshLensFromComponent.
static float   g_lensWrittenQuat[4] = { 0.0f, 0.0f, 0.0f, 0.0f };
static int32_t g_lensWrittenPosFP[3] = { 0, 0, 0 };
static std::atomic<int> g_lensWrittenValid{0};

// THE HEAD, WRITTEN INTO THE LENS THE MAIN VIEW IS RENDERED FROM.
//
// MAIN's own component renders nothing during a takeover -- the engine renders the main view through the
// taken-over camera, which is why writing THAT component's fov through RTTI widened the picture while the
// port's own fov force never reached it. So MAIN's orientation cannot be given to MAIN's component: with
// the second eye driven from the lens and nothing driving the lens, the left eye followed the head and the
// right eye did not, reported exactly that way.
//
// The position carries the viewpoint slider (CyberpunkVR_DevCamOffset*, yaw-rotated the same way
// PatchCamera applies it to the second eye) so the slider moves the PICTURE, not one eye, and the eye
// separation is MAIN's half -- BdPushTransformOnce computes it from the same numbers the second eye uses,
// with the engine's own change notification after the write.
// DEFAULT 0. Writing the composition into the lens COMPONENT does not turn the main view -- tried and
// reported "не работает" -- because the view is built from the located buffer, not from that component's
// rotation (its fov IS read from there, which is why the fov fix works at the component). Kept as a live
// key rather than deleted, since it is one line away from being the right answer if the route ever changes.
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_LensHeadWrite = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugLensHeadWrites = 0;

void PushLensHeadTransform(const float* quat) {
    if (!CyberpunkVR_LensHeadWrite) return;
    const uintptr_t comp = g_lensComp.load(std::memory_order_relaxed);
    if (comp <= 0x10000 || comp >= 0x7FF000000000ull || (comp & 7u) != 0) return;
    if (!quat || !IsPlausibleUnitQuaternion(quat)) return;
    if (!g_devCamPosValid.load(std::memory_order_acquire)) return;

    int32_t base[3] = {};
    for (int i = 0; i < 3; ++i) base[i] = g_devCamPosFP[i].load(std::memory_order_relaxed);
    const float ox = CyberpunkVR_DevCamOffsetX;
    const float oy = CyberpunkVR_DevCamOffsetY;
    const float oz = CyberpunkVR_DevCamOffsetZ;
    if (ox != 0.0f || oy != 0.0f || oz != 0.0f) {
        const float yaw = g_devCamAimValid.load(std::memory_order_acquire) ? g_devCamAimYaw : 0.0f;
        const float sy = sinf(yaw), cy = cosf(yaw);
        base[0] += static_cast<int32_t>((ox * cy - oy * sy) * 131072.0f);
        base[1] += static_cast<int32_t>((ox * sy + oy * cy) * 131072.0f);
        base[2] += static_cast<int32_t>(oz * 131072.0f);
    }

    BdPushTransformOnce(comp, quat[0], quat[1], quat[2], quat[3], false, base);
    ++CyberpunkVR_DebugLensHeadWrites;

    // Remember what is actually in the component now, rather than what we meant to put there: the write
    // goes through the same fixed-point rounding and IPD split, and the comparison has to be exact.
    int32_t rp[3] = {};
    bool okp = true;
    for (int i = 0; i < 3 && okp; ++i) {
        uint32_t v = 0;
        okp = ReadU32Safe(comp + 0xE0 + i * 4, &v);
        rp[i] = static_cast<int32_t>(v);
    }
    float rq[4] = {};
    if (okp && ReadFloatArraySafe(reinterpret_cast<const float*>(comp + 0xF0), rq, 4)) {
        for (int i = 0; i < 3; ++i) g_lensWrittenPosFP[i] = rp[i];
        for (int i = 0; i < 4; ++i) g_lensWrittenQuat[i] = rq[i];
        g_lensWrittenValid.store(1, std::memory_order_release);
    }
}

void RefreshLensFromComponent() {
    const uintptr_t comp = g_lensComp.load(std::memory_order_relaxed);
    if (comp <= 0x10000 || comp >= 0x7FF000000000ull || (comp & 7u) != 0) return;
    int32_t p[3] = {};
    bool ok = true;
    for (int i = 0; i < 3 && ok; ++i) {
        uint32_t v = 0;
        ok = ReadU32Safe(comp + 0xE0 + i * 4, &v);
        p[i] = static_cast<int32_t>(v);
    }
    // ...UNLESS IT IS OUR OWN WRITE COMING BACK. The head is composed into this same component below
    // (PushLensHeadTransform), so a blind re-read would take our composition for the mount's own aim and
    // apply the head twice, compounding every frame. Bit-for-bit equality is the test the port already
    // uses for the device camera's aim, and it is exact: only our own store reproduces it.
    const bool oursPos = g_lensWrittenValid.load(std::memory_order_acquire) != 0 &&
                         p[0] == g_lensWrittenPosFP[0] && p[1] == g_lensWrittenPosFP[1] &&
                         p[2] == g_lensWrittenPosFP[2];
    if (ok && !oursPos) {
        for (int i = 0; i < 3; ++i) g_devCamPosFP[i].store(p[i], std::memory_order_relaxed);
        g_devCamPosValid.store(1, std::memory_order_release);
    }
    if (ok) {
        // THE HEARTBEAT. DeviceCamActive() -- which gates the lens copy, the lens orientation, VRIK's
        // stand-down and the body mount -- believes a takeover only while this stamp is under 300 ms old,
        // and the only thing that sets it is the classifier claiming a camera. The turret's camera never
        // reaches that classifier (measured: not one claim in a session), so the whole path stayed off
        // even with the lens correctly bound: the second eye kept the engine's own position, the player's
        // head, with nothing but the eye separation added. A lens re-read this frame IS the liveness the
        // window is asking about, so it stamps.
        StampDeviceCam();
    }
    float q[4] = {};
    const bool haveQ = ReadFloatArraySafe(reinterpret_cast<const float*>(comp + 0xF0), q, 4) &&
                       IsPlausibleUnitQuaternion(q);
    const bool oursQuat = haveQ && g_lensWrittenValid.load(std::memory_order_acquire) != 0 &&
                          q[0] == g_lensWrittenQuat[0] && q[1] == g_lensWrittenQuat[1] &&
                          q[2] == g_lensWrittenQuat[2] && q[3] == g_lensWrittenQuat[3];
    if (haveQ && !oursQuat) {
        g_devCamBase[0] = q[0]; g_devCamBase[1] = q[1];
        g_devCamBase[2] = q[2]; g_devCamBase[3] = q[3];
        const float fx = 2.0f * (q[0] * q[1] - q[2] * q[3]);
        const float fy = 1.0f - 2.0f * (q[0] * q[0] + q[2] * q[2]);
        const float fz = 2.0f * (q[1] * q[2] + q[0] * q[3]);
        const float fh = sqrtf(fx * fx + fy * fy);
        g_devCamAimYaw = atan2f(-fx, fy);
        g_devCamAimPitch = atan2f(fz, fh);
        g_devCamAimValid.store(1, std::memory_order_release);
        g_devCamBaseValid.store(1, std::memory_order_release);
    }
}


std::atomic<int> g_remoteCamOn{0};
std::atomic<int32_t> g_remoteCamPosFP[3] = {};
// How close a cameraComponent has to sit to the published camera to be believed. The published point is
// the ENTITY's origin and the component sits at the lens, so this is not a few centimetres; cameras in
// the game stand metres apart, so 1.5 m separates them without being tight enough to miss the mount.
// ...AND IT IS A KNOB NOW, because 1.5 m was measured against a camera bolted to a wall. On the AV
// turret the mount flies: the script publishes the lens position every frame, but a frame of travel plus
// the tick the value spends in flight is metres, and the claim simply never happened -- measured
// DebugPatchCamDevice = 0 for a whole session in that seat, which took the fov, the lens and the second
// eye's base with it. The latch is sticky (one camera per takeover), so a generous FIRST claim costs
// nothing: after it the identity is fixed by address.
// 1.5 m, the value measured against a wall-mounted camera. It is a FALLBACK only: with an entity id
// published the identity decides and this is not consulted, so widening it buys nothing.
extern "C" __declspec(dllexport) float CyberpunkVR_DevCamTolM = 1.5f;
extern "C" __declspec(dllexport) unsigned int CyberpunkVR_DebugPatchCamDevice = 0;
static std::atomic<uintptr_t> g_camObjDevice{0};
static bool DeviceCamClaimed() { return g_camObjDevice.load(std::memory_order_relaxed) != 0; }
std::atomic<int> g_bdActive{0};
std::atomic<int> g_bdWantFovMilli{0};
std::atomic<int32_t> g_bdScenePosFP[3] = {};
float g_bdSceneQuat[4] = { 0.0f, 0.0f, 0.0f, 1.0f };
std::atomic<int> g_bdScenePoseValid{0};
std::atomic<int> g_bdCamFound{0};
std::atomic<int> g_playerCamOn{0};
std::atomic<int32_t> g_playerCamPosFP[3] = {};
static constexpr float kPlayerCamTolM = 2.0f;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMainCamRejects = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugVrcamCamRejects = 0;
// The RTT component the ENGINE renders the second view through, latched by the view-create detour in
// src/Stereo/FrameGraph.cpp. With two authored sets of vrcam components -- the player's and the
// braindance replacer's -- the name alone no longer identifies one object, and this does.
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugRttComp;
// How close a patched object has to be to the pose script reports for the scene camera. The orientation
// is the discriminating half -- 0.99 is ~16 degrees, and almost nothing in the world is aimed the way a
// cinematic camera is -- while the distance covers the frame of lag between the script read and this
// write, at replay speeds a few centimetres.
static constexpr float kBdPosTolM = 0.60f;
static constexpr float kBdDotTol  = 0.995f;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugBdCandidates = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugBdCamHits = 0;
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
void StampDeviceCam() {
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
// IS THIS THE CAMERA OF THE PLAYER WHO IS LIVE RIGHT NOW. The same fixed-point read as the device
// gate below, against the point script publishes for Game.GetPlayer()'s camera component. An unreadable
// position is NOT a rejection: dropping MAIN on a bad read would leave the head with no camera at all.
static bool PlayerCamPositionMatches(uintptr_t obj) {
    const uintptr_t posAddr = obj + 0xE0;
    int32_t p[3] = {};
    for (int i = 0; i < 3; ++i) {
        uint32_t v = 0;
        if (!ReadU32Safe(posAddr + i * 4, &v)) return true;
        p[i] = static_cast<int32_t>(v);
    }
    const float k = 1.0f / 131072.0f;
    float d2 = 0.0f;
    for (int i = 0; i < 3; ++i) {
        const float d = (p[i] - g_playerCamPosFP[i].load(std::memory_order_relaxed)) * k;
        d2 += d * d;
    }
    return d2 <= (kPlayerCamTolM * kPlayerCamTolM);
}

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
    const float tol = (CyberpunkVR_DevCamTolM > 0.05f && CyberpunkVR_DevCamTolM < 100.0f)
                          ? CyberpunkVR_DevCamTolM : 1.5f;
    return d2 <= (tol * tol);
}

bool DeviceCamActive() {
    if (!CyberpunkVR_DeviceCamFollow) return false;
    // EITHER GATE, AND BOTH MEAN "A CAMERA THAT IS NOT THE PLAYER'S IS LATCHED". A takeover publishes
    // the position of one (g_remoteCamOn); a braindance is only claimed once the pose match below has
    // actually FOUND the scene's camera (g_bdCamFound).
    //
    // The distinction is not pedantic: this answer suspends VRIK, takes head translation away from the
    // second eye and disables the body mount. Raising it on the mere fact that a braindance was running
    // switched all of that off while the port still had no camera to steer -- the head lost the view
    // instead of gaining it.
    if (!g_remoteCamOn.load(std::memory_order_relaxed) &&
        !g_bdCamFound.load(std::memory_order_relaxed)) return false;
    const unsigned long long t = g_deviceCamLastMs.load(std::memory_order_relaxed);
    if (t == 0) return false;
    return (GetTickCount64() - t) < 300ull;
}
extern "C" unsigned long long CyberpunkVR_VrcamCamNameHash();   // stereo/sync_stereo.cpp
// Read by the braindance census: whether the second view is producing frame-graph nodes and whether its
// render target is being captured. Both live in the stereo translation unit.
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugVrcamNodeHits;
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugMirrorRtvHits;
// The same component on the braindance replacer, which carries its own prefix. Only one of the two is
// ever enabled, so either name means "the second view".
extern "C" unsigned long long CyberpunkVR_VrcamBdCamNameHash();

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

// THE LATCH IS RE-VALIDATED BY NAME, NOT BY POSITION.
//
// The check this replaces asked whether the latched object still sits where the script says the
// player's camera is. During a replay those are legitimately different places, so the test failed and
// the latch was thrown away -- and a thrown-away latch means `return 0`, which means the camera write
// for that pass DOES NOT HAPPEN AT ALL. Measured in playback, over 7.68 s:
//
//     MainCamRejects +33, VrcamCamRejects +33   ->  4.3 dropped writes a second, in both eyes
//
// Four skipped writes a second is the "подергивает" that survived every source fix: the pose was
// correct, it simply was not applied on those passes. The name does not move, so one qword read keeps
// the original intent -- notice when the object stops being MAIN -- without a test a replay is
// guaranteed to fail.
static bool CamNameStillMain(uintptr_t obj) {
    const int off = g_camNameOffset.load(std::memory_order_acquire);
    if (off < 0) return true;              // not calibrated yet: do not drop a latch over it
    uint64_t nm = 0;
    if (!ReadU64Safe(obj + off, &nm)) return true;
    return nm == kCamNameMain;
}

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
    // A SURVEILLANCE CAMERA IS `cameraComponent`; A BRAINDANCE CAMERA IS WHATEVER THE SCENE USES, and its
    // name is exactly the thing this project could not find out from either side -- which is why it is
    // identified by pose instead. So the liveness proof for the write is a NON-ZERO CName at the
    // calibrated offset (the entity can be unloaded between the last patch and the release, and that is
    // what this check exists to catch), and the stricter name test still applies to a takeover.
    if (name == 0) return;
    if (!g_bdCamFound.load(std::memory_order_relaxed) &&
        name != cvr::cname_hash("cameraComponent")) return;
    float cur = 0.0f;
    if (!ReadFloatSafe(obj + 0x128, &cur)) return;
    if (!(g_devCamFovOrig > 1.0f && g_devCamFovOrig < 179.0f)) return;
    WriteFloatSafe(obj + 0x128, g_devCamFovOrig);
    if (g_verboseLog) Log("PatchCamera: device camera fov handed back %.3f -> %.3f\n", cur, g_devCamFovOrig);
}


// ===== BRAINDANCE: WHICH OBJECT IS THE CAMERA THE SCENE RENDERS THROUGH =========================
//
// Called for every object the classifier does not recognise, from inside the camera writer, so the
// object's own orientation is already in hand and costs nothing to test. That ordering is the whole
// performance story: the site fires ~16M times a session for ordinary placed components, so the
// ORIENTATION is tested first -- four multiplies, no memory read -- and the position is only read for
// the handful of objects aimed the way the scene's camera is.
bool BraindanceCameraMatch(uintptr_t obj, const float* quat) {
    if (!CyberpunkVR_DeviceCamFollow) return false;
    if (!g_bdActive.load(std::memory_order_relaxed)) return false;
    if (!g_bdScenePoseValid.load(std::memory_order_acquire)) return false;
    if (!obj || obj < 0x10000 || !quat) return false;

    float dot = quat[0] * g_bdSceneQuat[0] + quat[1] * g_bdSceneQuat[1] +
                quat[2] * g_bdSceneQuat[2] + quat[3] * g_bdSceneQuat[3];
    if (dot < 0.0f) dot = -dot;                 // q and -q are the same rotation

    // THE CENSUS, one line a second, and it is written to be informative when NOTHING matches: the best
    // orientation agreement seen, the distance and component name of the best candidate, and the
    // per-second deltas that say whether the second view is rendering and being captured at all.
    static uint64_t s_lastMs = 0;
    static float s_bestDot = 0.0f;
    static float s_bestDist = 1.0e9f;
    static uint64_t s_bestName = 0;
    static uintptr_t s_bestObj = 0;
    static uint64_t s_seen = 0;
    static uint64_t p_main = 0, p_vrcam = 0, p_dev = 0, p_nodes = 0, p_rtv = 0;
    static uint64_t s_shortName[8] = {};
    static float s_shortDist[8] = {};
    static float s_shortFov[8] = {};
    static int s_shortN = 0;
    ++s_seen;

    float dist = -1.0f;
    if (dot >= 0.95f) {
        int32_t p[3] = {};
        bool ok = true;
        for (int i = 0; i < 3 && ok; ++i) {
            uint32_t v = 0;
            ok = ReadU32Safe(obj + 0xE0 + i * 4, &v);
            p[i] = static_cast<int32_t>(v);
        }
        if (ok) {
            const float k = 1.0f / 131072.0f;
            float d2 = 0.0f;
            for (int i = 0; i < 3; ++i) {
                const float d = (p[i] - g_bdScenePosFP[i].load(std::memory_order_relaxed)) * k;
                d2 += d * d;
            }
            dist = sqrtf(d2);
        }
    }

    // A SHORTLIST OF WHAT IS OUT THERE, so one braindance is enough to identify the right object rather
    // than one build per guess: every distinct component name that comes within a metre of the scene
    // camera's pose, with the fov field that decides whether it is a camera at all.
    if (dot >= 0.99f && dist >= 0.0f && dist <= 1.5f) {
        const int off = g_camNameOffset.load(std::memory_order_acquire);
        uint64_t nm = 0;
        if (off >= 0) ReadU64Safe(obj + off, &nm);
        float fv = -1.0f;
        ReadFloatSafe(obj + 0x128, &fv);
        bool have = false;
        for (int i = 0; i < s_shortN; ++i)
            if (s_shortName[i] == nm) { have = true;
                                       if (dist < s_shortDist[i]) { s_shortDist[i] = dist; s_shortFov[i] = fv; }
                                       break; }
        if (!have && s_shortN < 8) {
            s_shortName[s_shortN] = nm; s_shortDist[s_shortN] = dist; s_shortFov[s_shortN] = fv;
            ++s_shortN;
        }
    }

    if (dot > s_bestDot || (dist >= 0.0f && dist < s_bestDist)) {
        if (dot > s_bestDot) s_bestDot = dot;
        if (dist >= 0.0f && dist < s_bestDist) {
            s_bestDist = dist;
            s_bestObj = obj;
            const int off = g_camNameOffset.load(std::memory_order_acquire);
            uint64_t nm = 0;
            if (off >= 0) ReadU64Safe(obj + off, &nm);
            s_bestName = nm;
        }
    }

    {
        const uint64_t now = GetTickCount64();
        if (s_lastMs == 0) s_lastMs = now;
        else if (now - s_lastMs >= 1000ull) {
            s_lastMs = now;
            const uint64_t c_main  = CyberpunkVR_DebugPatchCamMain;
            const uint64_t c_vrcam = CyberpunkVR_DebugPatchCamVrcam;
            const uint64_t c_dev   = CyberpunkVR_DebugPatchCamDevice;
            const uint64_t c_nodes = CyberpunkVR_DebugVrcamNodeHits;
            const uint64_t c_rtv   = CyberpunkVR_DebugMirrorRtvHits;
            if (g_verboseLog) Log("[bd] best dot=%.4f dist=%.2fm name=0x%016llX obj=%p unknowns=%llu | latched=%p "
                "found=%d | patch main=+%llu vrcam=+%llu dev=+%llu | vrcamNodes=+%llu rtv=+%llu"
                " | rejects main=%llu vrcam=%llu rtt=%p\n",
                static_cast<double>(s_bestDot),
                static_cast<double>(s_bestDist >= 1.0e8f ? -1.0f : s_bestDist),
                static_cast<unsigned long long>(s_bestName),
                reinterpret_cast<void*>(s_bestObj),
                static_cast<unsigned long long>(s_seen),
                reinterpret_cast<void*>(g_camObjDevice.load(std::memory_order_relaxed)),
                g_bdCamFound.load(std::memory_order_relaxed),
                static_cast<unsigned long long>(c_main  - p_main),
                static_cast<unsigned long long>(c_vrcam - p_vrcam),
                static_cast<unsigned long long>(c_dev   - p_dev),
                static_cast<unsigned long long>(c_nodes - p_nodes),
                static_cast<unsigned long long>(c_rtv   - p_rtv),
                static_cast<unsigned long long>(CyberpunkVR_DebugMainCamRejects),
                static_cast<unsigned long long>(CyberpunkVR_DebugVrcamCamRejects),
                reinterpret_cast<void*>(static_cast<uintptr_t>(CyberpunkVR_DebugRttComp)));
            for (int i = 0; i < s_shortN; ++i)
                if (g_verboseLog) Log("[bd]   near the scene camera: name=0x%016llX dist=%.3fm fov=%.3f\n",
                    static_cast<unsigned long long>(s_shortName[i]),
                    static_cast<double>(s_shortDist[i]), static_cast<double>(s_shortFov[i]));
            s_shortN = 0;
            p_main = c_main; p_vrcam = c_vrcam; p_dev = c_dev; p_nodes = c_nodes; p_rtv = c_rtv;
            s_bestDot = 0.0f; s_bestDist = 1.0e9f; s_bestName = 0; s_bestObj = 0; s_seen = 0;
        }
    }

    if (dot < kBdDotTol || dist < 0.0f || dist > kBdPosTolM) return false;

    // AND IT HAS TO BE A CAMERA. The pose alone is not enough: the first build of this matcher latched
    // the object at 0.155 m with dot 0.9904 and wrote the head, the aim and the fov into it for a whole
    // braindance -- and its name hash, reversed against the exe's own strings, was `Senses`. A senses
    // component rides the head, so it sits exactly where the camera does. What only a camera has is a
    // plausible FOV at +0x128 equal to the one script reports for the active camera (or to ours, once we
    // have forced it: the latch survives by address, but a re-match after a drop must still succeed).
    {
        float camFov = 0.0f;
        if (!ReadFloatSafe(obj + 0x128, &camFov)) return false;
        if (!(camFov > 1.0f && camFov < 179.0f)) return false;
        const float scriptFov =
            static_cast<float>(g_bdWantFovMilli.load(std::memory_order_relaxed)) * 0.001f;
        const float ours = g_normalFovOverrideValue;
        const bool fovAgrees =
            (scriptFov > 1.0f && fabsf(camFov - scriptFov) < 0.75f) ||
            (ours > 1.0f && fabsf(camFov - ours) < 0.75f) ||
            (g_devCamFovSaved.load(std::memory_order_relaxed) &&
             fabsf(camFov - g_devCamFovOrig) < 0.75f);
        if (!fovAgrees) return false;
    }

    const uintptr_t prev = g_camObjDevice.exchange(obj, std::memory_order_relaxed);
    if (prev != obj) {
        const int off = g_camNameOffset.load(std::memory_order_acquire);
        uint64_t nm = 0;
        if (off >= 0) ReadU64Safe(obj + off, &nm);
        if (g_verboseLog) Log("PatchCamera: BRAINDANCE camera %p (was %p) name=0x%016llX dot=%.4f dist=%.3fm\n",
            reinterpret_cast<void*>(obj), reinterpret_cast<void*>(prev),
            static_cast<unsigned long long>(nm),
            static_cast<double>(dot), static_cast<double>(dist));
        g_devCamBaseValid.store(0, std::memory_order_relaxed);   // a different camera: re-latch its aim
        g_devCamPosValid.store(0, std::memory_order_relaxed);
    }
    ++CyberpunkVR_DebugBdCandidates;
    g_bdCamFound.store(1, std::memory_order_release);
    StampDeviceCam();
    return true;
}

// The braindance ended. Hand the fov back to the object we forced it into, then forget it -- nothing of
// this braindance may be reused by whatever is rendered next.
// THE TAKEOVER'S OBJECTS, DROPPED. Called from the gate the script drives (VRRemoteCamera(0)); see the
// note there for why each of the three has to go.
void TakeoverLensRelease() {
    g_lensWrittenValid.store(0, std::memory_order_relaxed);
    const uintptr_t lens = g_lensComp.exchange(0, std::memory_order_relaxed);
    g_takeoverEntity.store(0, std::memory_order_relaxed);
    g_camObjDevice.store(0, std::memory_order_relaxed);
    if (lens && g_verboseLog) Log("PatchCamera: lens released (was %p)\n", reinterpret_cast<void*>(lens));
}

void BraindanceCameraRelease() {
    if (!g_bdCamFound.load(std::memory_order_relaxed)) return;
    DeviceCamRestoreFov();                       // reads g_camObjDevice, so BEFORE it is cleared
    g_bdCamFound.store(0, std::memory_order_relaxed);
    g_camObjDevice.store(0, std::memory_order_relaxed);
    g_devCamPosValid.store(0, std::memory_order_relaxed);
    g_devCamBaseValid.store(0, std::memory_order_relaxed);
    g_devCamViewValid.store(0, std::memory_order_relaxed);
    g_devCamAimValid.store(0, std::memory_order_relaxed);
    if (g_verboseLog) Log("PatchCamera: braindance camera released\n");
}

// ARM THE FAST PATH, from the classifier, on a camera it recognised. Nothing here is a policy
// decision the trampoline could make for itself: the trampoline can only compare pointers, and the
// question of WHICH pointers are the two cameras right now is what this function answers.
//
// The conditions are the conservative ones. Both cameras must have been recognised since the last
// disarm, or a single-camera moment (menu, load, VRCAM disabled) would arm a filter that then
// rejects the camera that had not appeared yet. The name offset must be calibrated, because until
// it is the slow path is still learning where the CName lives. And never during a braindance: the
// camera hunt there tests EVERY object and a filter would starve it.
//
// Written only on an actual change, so an armed steady state produces no store traffic at all on a
// line three job threads are reading every call.
static void patch_fast_note(int kind) {
    if (kind == 1) g_pfSeenMain = true;
    else if (kind == 2) g_pfSeenVrcam = true;
    if (!g_pfSeenMain || !g_pfSeenVrcam) return;
    if (g_camNameOffset.load(std::memory_order_acquire) < 0) return;
    if (g_bdActive.load(std::memory_order_relaxed)) return;
    // ...AND NOT WHILE A TAKEOVER'S LENS IS STILL BEING LOOKED FOR, for the same reason as the braindance
    // hunt: the lens is reached through the ENTITY, the entity is reached through any component of it, and
    // a filter that only admits the two cameras admits none of them. Measured in the turret with the
    // filter armed: owner[2] = 0, not one component of the AV reached the classifier, no bind happened.
    // Once the lens is bound the filter may arm again -- the lens is read directly from then on.
    // ...BUT A CLAIMED CAMERA NEEDS NO HUNT, and without this the filter stayed disarmed for the whole
    // takeover of a wall camera: that one IS claimed through this hook, so the lens is never looked up in
    // an entity's component list and g_lensComp stays 0 forever. The filter is what keeps this site cheap
    // -- it is UpdateWorldTransforms, ~10 200 calls a second over every component in the world -- so
    // leaving it off for minutes is a real cost, and it was one this pass found by reading the condition
    // rather than the picture.
    if (g_remoteCamOn.load(std::memory_order_relaxed) &&
        g_lensComp.load(std::memory_order_relaxed) == 0 && !DeviceCamClaimed()) return;
    const uintptr_t m = g_camObjMain.load(std::memory_order_relaxed);
    const uintptr_t v = g_camObjVrcam.load(std::memory_order_relaxed);
    const uintptr_t d = g_camObjDevice.load(std::memory_order_relaxed);
    if (!m || !v) return;
    if (g_patchFast.owner[0] != m) g_patchFast.owner[0] = m;
    if (g_patchFast.owner[1] != v) g_patchFast.owner[1] = v;
    if (g_patchFast.owner[2] != d) g_patchFast.owner[2] = d;
    if (g_patchFast.armed != 1u) g_patchFast.armed = 1u;
}

static thread_local int s_vrcamMismatch = 0;
static thread_local int s_mainMismatch = 0;

int ClassifyPatchCameraOwner(void* ownerState) {
    const uintptr_t obj = reinterpret_cast<uintptr_t>(ownerState);
    if (!obj || obj < 0x10000) return 0;

    // Fast path: the overwhelming majority of calls end here.
    // THE FAST PATHS GIVE THE LATCH UP when the object stops being the right one. Without this a
    // braindance keeps whichever candidate was latched first for the whole scene, and the slow path
    // below -- where both identities are actually tested -- is never reached again.
    if (obj == g_camObjMain.load(std::memory_order_relaxed)) {
        if (!g_bdActive.load(std::memory_order_relaxed) ||
            !g_playerCamOn.load(std::memory_order_relaxed) ||
            CamNameStillMain(obj)) {
            ++CyberpunkVR_DebugPatchCamMain;
            patch_fast_note(1);
            return 1;
        }
        g_camObjMain.store(0, std::memory_order_relaxed);
        PatchFastDisarm();   // the latch is gone: the slow path has to find the camera again
        ++CyberpunkVR_DebugMainCamRejects;
    }
    if (obj == g_camObjVrcam.load(std::memory_order_relaxed)) {
        const uintptr_t rtt = static_cast<uintptr_t>(CyberpunkVR_DebugRttComp);
        if (!rtt || obj == rtt) {
            s_vrcamMismatch = 0;
            ++CyberpunkVR_DebugPatchCamVrcam;
            patch_fast_note(2);
            return 2;
        }
        // ONE MISMATCH IS NOT A REASON TO DROP THE WRITE. CyberpunkVR_DebugRttComp is written by the
        // view-create hook on the render thread and read here on the game thread, so it can disagree
        // for a pass without the latch being wrong -- and dropping the latch costs the camera write for
        // that pass, not merely the identification. Measured at 4.3 such passes a second in playback,
        // which is what the second eye was jerking on. A real replacement persists, so it still gets
        // noticed within a few passes.
        if (++s_vrcamMismatch < 4) {
            ++CyberpunkVR_DebugPatchCamVrcam;
            patch_fast_note(2);
            return 2;
        }
        s_vrcamMismatch = 0;
        g_camObjVrcam.store(0, std::memory_order_relaxed);
        PatchFastDisarm();
        ++CyberpunkVR_DebugVrcamCamRejects;
    }
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
            const uint64_t vrcamBd = CyberpunkVR_VrcamBdCamNameHash();
            if (v == kCamNameMain || (vrcam != 0 && v == vrcam) || (vrcamBd != 0 && v == vrcamBd)) {
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
        // TWO OBJECTS ANSWER TO THIS NAME IN A BRAINDANCE -- the player's camera and the replacer's --
        // so inside one the position script publishes for the LIVE player's camera decides. Outside a
        // braindance nothing is published and nothing is rejected, which is the old behaviour exactly.
        // ...AND A SINGLE MISMATCH IS NOT ENOUGH, because a rejection here skips the camera write for
        // this pass, not just the identification. The position script publishes is a tick old, so the
        // right object fails this test transiently whenever the camera is moving; the WRONG object --
        // the replacer's camera, metres away -- fails it every single pass. Requiring the failure to
        // repeat keeps the discrimination and stops costing writes: measured at 4.3 dropped writes a
        // second in playback before this, in both eyes, which is what the pair was jerking on.
        if (g_bdActive.load(std::memory_order_relaxed) &&
            g_playerCamOn.load(std::memory_order_relaxed) &&
            !PlayerCamPositionMatches(obj)) {
            if (++s_mainMismatch >= 4) {
                s_mainMismatch = 0;
                ++CyberpunkVR_DebugMainCamRejects;
                ++CyberpunkVR_DebugPatchCamOther;
                return 0;
            }
        } else {
            s_mainMismatch = 0;
        }
        {
            const uintptr_t prev = g_camObjMain.load(std::memory_order_relaxed);
            if (prev != obj && prev != 0 && g_bdActive.load(std::memory_order_relaxed))
                if (g_verboseLog) Log("PatchCamera: MAIN camera is now %p (was %p)\n",
                    reinterpret_cast<void*>(obj), reinterpret_cast<void*>(prev));
        }
        g_camObjMain.store(obj, std::memory_order_relaxed);   // latch for the fast path
        ++CyberpunkVR_DebugCamRebinds;
        ++CyberpunkVR_DebugPatchCamMain;
        patch_fast_note(1);
        return 1;
    }
    const uint64_t vrcamBd = CyberpunkVR_VrcamBdCamNameHash();
    if ((vrcam != 0 && name == vrcam) || (vrcamBd != 0 && name == vrcamBd)) {
        // THE NAME NO LONGER IDENTIFIES ONE OBJECT. Two sets of these components are authored -- the
        // player's `vrcam_<W>x<H>` and the replacer's `vrcam_braindance_<W>x<H>` -- and only one of them
        // is enabled at a time, so accepting both names without a tie-break made this latch alternate
        // between the component that renders and one that does not. Every dispatch that landed on the
        // wrong one left the rendering component with the engine's own pose instead of ours: the eye
        // jumps between the two by the size of the head offset, reported as "дергает на IPD".
        //
        // The engine answers it directly: the RTT view-create detour records the component it is
        // building the view from. When that is known it is the identity; before the first view-create
        // the name still stands in.
        const uintptr_t rtt = static_cast<uintptr_t>(CyberpunkVR_DebugRttComp);
        if (rtt && obj != rtt) {
            ++CyberpunkVR_DebugVrcamCamRejects;
            ++CyberpunkVR_DebugPatchCamOther;
            return 0;
        }
        {
            const uintptr_t prev = g_camObjVrcam.load(std::memory_order_relaxed);
            if (prev != obj && prev != 0)
                if (g_verboseLog) Log("PatchCamera: second view is now %p (was %p) name=0x%016llX rtt=%p\n",
                    reinterpret_cast<void*>(obj), reinterpret_cast<void*>(prev),
                    static_cast<unsigned long long>(name), reinterpret_cast<void*>(rtt));
        }
        g_camObjVrcam.store(obj, std::memory_order_relaxed);
        ++CyberpunkVR_DebugCamRebinds;
        ++CyberpunkVR_DebugPatchCamVrcam;
        return 2;
    }
    if (CyberpunkVR_DeviceCamFollow && g_remoteCamOn.load(std::memory_order_relaxed)) {
        static const uint64_t kCamNameDevice = cvr::cname_hash("cameraComponent");
        const bool nameOk = (name == kCamNameDevice) || (CyberpunkVR_DevCamAnyName != 0);

        // A STICKY LATCH: ONE CAMERA PER TAKEOVER, DECIDED ONCE.
        //
        // Re-validating the held camera by distance did not hold -- measured on the AV turret, the lens
        // changed identity 154643 times in one session, cycling between the cameras the AV carries. The
        // published position is a tick old and the mount is flying, so the camera we already hold fails
        // its own distance test while a sibling passes it. Every swap clears the pose flags, re-stamps,
        // and overwrites g_devCamFovOrig with the fov WE forced -- which is why the handback line came out
        // as "110.000 -> 110.000" with the mount's real 60 lost.
        //
        // So the first camera that qualifies keeps the claim for the whole takeover; it is released when
        // the takeover ends (VRRemoteCamera(0) -> BraindanceCameraRelease). Others are not the lens, and
        // that is a decision, not a measurement to repeat every frame.
        {
            const uintptr_t cur = g_camObjDevice.load(std::memory_order_relaxed);
            if (cur != 0) {
                if (obj != cur) {
                    ++CyberpunkVR_DebugPatchCamOther;
                    return 0;
                }
                // The camera we hold: keep the lens pose fresh, do not re-claim.
                ++CyberpunkVR_DebugPatchCamDevice;
                StampDeviceCam();
                return 3;
            }
        }

        // THE ENTITY FIRST, THE PLACE ONLY AS A FALLBACK. With an id published the test is exact and
        // motion cannot break it; without one (nothing published yet, or an older script) the old
        // position test still decides, so a surveillance camera behaves exactly as before.
        const uint64_t wantId = g_takeoverEntityId.load(std::memory_order_relaxed);
        bool identified = false;
        if (wantId != 0) {
            // THE ENTITY IS THE HANDLE TO THE LENS, and ANY component of it is enough to reach it -- but
            // only the camera may become the latch. Measured at the turret: the first component of the AV
            // to arrive here matched the entity and was claimed, and it was not a camera at all (name
            // 0x86C64F09202D3BEA, its position 33 m from the lens), because xr_dev_cam_any_name drops the
            // name test. The second eye then took its lens pose from that object. So the name is required
            // again, and the owner is kept: whatever arrives, the lens itself is looked up in the
            // entity's component list, which does not depend on the lens ever reaching this hook.
            uint64_t owner = 0, oid = 0;
            const bool ownerOk = ReadU64Safe(obj + 0x50, &owner) && owner > 0x10000 &&
                                 ReadU64Safe(static_cast<uintptr_t>(owner) + 0x48, &oid);
            if (ownerOk && oid == wantId) {
                const uintptr_t ent = static_cast<uintptr_t>(owner);
                if (g_takeoverEntity.exchange(ent, std::memory_order_relaxed) != ent)
                    BindLensFromEntity(ent);
                identified = (name == kCamNameDevice);
            }
        } else {
            identified = nameOk && DeviceCamPositionMatches(obj);
        }
        if (identified) {
            const uintptr_t prev = g_camObjDevice.exchange(obj, std::memory_order_relaxed);
            if (prev != obj) {
                // Logged on every change of identity, and that is the diagnostic this build exists to
                // produce: ONE address while a takeover is on and nothing in between is what keying on
                // the name assumes. A stream of different addresses would mean the game patches other
                // cameras in the world too, and then the name alone is not enough.
                if (g_verboseLog) Log("PatchCamera: device camera component %p (was %p) hits=%u\n",
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
    // BRAINDANCE: the camera whose own fov is the one the script reports for the active camera. Matched
    // once and then latched by address, so the match cannot be lost the moment the fov is overwritten
    // with the headset's -- and handed to the device path, which already does everything that follows.
    if (CyberpunkVR_DeviceCamFollow && g_bdActive.load(std::memory_order_relaxed)) {
        const int wantMilli = g_bdWantFovMilli.load(std::memory_order_relaxed);
        if (wantMilli > 5000 && wantMilli < 179000) {
            float camFov = 0.0f;
            if (ReadFloatSafe(obj + 0x128, &camFov) && camFov > 1.0f && camFov < 179.0f) {
                const int gotMilli = static_cast<int>(camFov * 1000.0f);
                if (gotMilli > wantMilli - 30 && gotMilli < wantMilli + 30) {
                    const uintptr_t prev = g_camObjDevice.exchange(obj, std::memory_order_relaxed);
                    if (prev != obj) {
                        // ONE address per braindance is what this assumes. A stream of them would mean
                        // the fov is not an identity either, and then the next lever is the name hash
                        // this very line prints.
                        if (g_verboseLog) Log("PatchCamera: braindance camera %p (was %p) name=0x%016llX fov=%.3f "
                            "want=%.3f\n",
                            reinterpret_cast<void*>(obj), reinterpret_cast<void*>(prev),
                            static_cast<unsigned long long>(name), camFov,
                            static_cast<double>(wantMilli) * 0.001);
                    }
                    ++CyberpunkVR_DebugBdCamHits;
                    ++CyberpunkVR_DebugCamRebinds;
                    ++CyberpunkVR_DebugPatchCamDevice;
                    g_devCamBaseValid.store(0, std::memory_order_relaxed);
                    g_devCamPosValid.store(0, std::memory_order_relaxed);
                    StampDeviceCam();
                    return 3;
                }
            }
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


