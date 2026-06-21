#include <windows.h>
#include <psapi.h>
#include <xinput.h>
#include <cstdint>
#include <cstddef>
#include <cstdio>
#include <cstdarg>
#include <cstring>
#include <cmath>
#include <share.h>
#include "aob_scanner.h"
#include "live_controls_ui.h"
#include "launcher_dialog.h"
#include "openxr_manager.h"
#include "runtime_fov_correction.h"

#define CreateDXGIFactory RealDXGIHeader_CreateDXGIFactory
#define CreateDXGIFactory1 RealDXGIHeader_CreateDXGIFactory1
#define CreateDXGIFactory2 RealDXGIHeader_CreateDXGIFactory2
#define DXGIDeclareAdapterRemovalSupport RealDXGIHeader_DXGIDeclareAdapterRemovalSupport
#define DXGIDisableVBlankVirtualization RealDXGIHeader_DXGIDisableVBlankVirtualization
#define DXGIGetDebugInterface1 RealDXGIHeader_DXGIGetDebugInterface1
#define DXGIReportAdapterConfiguration RealDXGIHeader_DXGIReportAdapterConfiguration
#include "dxgi_factory_wrapper.h"
#undef CreateDXGIFactory
#undef CreateDXGIFactory1
#undef CreateDXGIFactory2
#undef DXGIDeclareAdapterRemovalSupport
#undef DXGIDisableVBlankVirtualization
#undef DXGIGetDebugInterface1
#undef DXGIReportAdapterConfiguration

HMODULE g_realDxgi = nullptr;
static FILE* g_logFile = nullptr;
static char g_gameDir[MAX_PATH] = {};
static char g_liveControlPath[MAX_PATH] = {};
static char g_launcherConfigPath[MAX_PATH] = {};
static char g_backendModulePath[MAX_PATH] = {};
static FILETIME g_lastLiveControlWrite = {};
static char g_hudLayoutPath[MAX_PATH] = {};
// Bridge files in the CET VRIK mod folder (CET sandboxes a mod's relative paths to
// its own folder). dxgi WRITES vrik_settings.ini (mouse-Y flag, CET reads it); CET
// WRITES vrik_recenter.ini (a counter on save load) which dxgi polls to recenter.
static char g_vrikSettingsPath[MAX_PATH] = {};
static char g_vrikRecenterPath[MAX_PATH] = {};
static FILETIME g_lastVrikRecenterWrite = {};
static const int kNoRecenterBaseline = -2000000000;
static int g_lastVrikRecenterCounter = kNoRecenterBaseline;

// HUD placement values = the 34 contiguous floats in LiveControlsUiState
// (xrHudScale .. xrHudOxygenBar). The CET HUD mod (CyberpunkVRPort_HUD)
// polls hud_layout.ini for these xr_hud_* keys; g_liveControls has no HUD
// fields, so we keep the last overlay-set values here and (de)serialize them.
// Order MUST match the struct field order so a single memcpy bridges them.
static const int kHudFieldCount = 34;
static const char* const kHudKeys[kHudFieldCount] = {
    "xr_hud_scale", "xr_hud_scale_y", "xr_hud_scale_scale",
    "xr_hud_phone", "xr_hud_phone_y", "xr_hud_phone_scale",
    "xr_hud_top_left_alerts", "xr_hud_top_left_alerts_y", "xr_hud_top_left_alerts_scale",
    "xr_hud_top_right", "xr_hud_top_right_y", "xr_hud_top_right_scale",
    "xr_hud_bottom_left", "xr_hud_bottom_left_y", "xr_hud_bottom_left_scale",
    "xr_hud_bottom_left_top", "xr_hud_bottom_left_top_y", "xr_hud_bottom_left_top_scale",
    "xr_hud_radio", "xr_hud_radio_y", "xr_hud_radio_scale",
    "xr_hud_bottom_right", "xr_hud_bottom_right_y", "xr_hud_bottom_right_scale",
    "xr_hud_right_center", "xr_hud_right_center_y", "xr_hud_right_center_scale",
    "xr_hud_johnny_hint", "xr_hud_activity_log", "xr_hud_warning",
    "xr_hud_boss_health", "xr_hud_vehicle_scan", "xr_hud_progress_bar", "xr_hud_oxygen_bar",
};
static float g_hudValues[kHudFieldCount];
static bool g_hudDefaultsInit = false;
static bool g_hudLoaded = false;

static void EnsureHudDefaults() {
    if (g_hudDefaultsInit) return;
    g_hudDefaultsInit = true;
    for (int i = 0; i < kHudFieldCount; ++i) {
        size_t n = strlen(kHudKeys[i]);
        bool isScale = n >= 6 && strcmp(kHudKeys[i] + n - 6, "_scale") == 0;
        g_hudValues[i] = isScale ? 1.0f : 0.0f; // scales default 1.0, offsets 0
    }
}
static uintptr_t g_gameModuleBase = 0;
static size_t g_gameModuleSize = 0;
void Log(const char* fmt, ...);
bool InstallFixLoDHook();

struct LiveControls {
    volatile float xrHeadOffsetX;
    volatile float xrHeadOffsetY;
    volatile float xrHeadOffsetZ;
    volatile int xrRecenter;
    volatile int xrMonoSubmit;
    volatile int xrAERSubmit;
    volatile float xrForceFov;
    volatile int xrMenuRect;
    volatile float xrMenuFov;
    volatile int xr3DofMovement;
    volatile int xrDLSSMatrixHook;
    volatile int xrDLSSSlotMode;
    volatile int xrDLSSLogStride;
    volatile int xrAERPairGate;
    volatile int xrAERStartEye;
    volatile int xrAERDebugEye;
    volatile float xrMotionPredictMs;
    volatile float xrStereoScale;
    volatile float xrWorldScale;   // uniform world scale (1.0 = default, <1 = world bigger)
    volatile float xrIpdScale;     // eye-separation multiplier on runtime IPD
    volatile float xrSharpness;    // CAS sharpen strength (0 = off .. 1)
    volatile float xrSharpmix;     // CAS sharpen mix (0..1)
    volatile int xrReuseLastFrame; // 1 = reuse last clean frame on stale AER ticks
    volatile int xrNvofPerf;       // NvOF perf level: 0=FAST, 1=MEDIUM, 2=SLOW (Higher quality AER v2)
    volatile int xrPairLock;       // 1 = freeze tracked pose per stereo pair (anti-tear). 0 = live pose every locate.
    volatile int xrRenderPoseSubmit;
    volatile int xrAERHalfRate;
    volatile int xrAERV2;
    volatile int xrPoseLag;
    volatile int xrRuntime;
    volatile int xrDepthSubmit;
    volatile int xrMovementControl; // 0 = Game heading, 1 = HMD head-oriented locomotion (legacy mirror of xrMovementSource)
    volatile int xrDisableMouseY;   // 1 = suppress mouse pitch (CET VRIK mod applies it)
    volatile int xrXInputHook;      // 1 = merge VR controller into XInput gamepad 0
    volatile int xrSnapTurn;        // 1 = discrete snap turn from right-stick X
    volatile float xrSnapTurnAngleDeg; // degrees per snap pulse
    volatile int xrMovementSource;  // 0 = Game, 1 = HMD, 2 = LeftHand, 3 = RightHand
    volatile int xrXInputInstall;   // 1 = install the XInput entry-point detour at startup (default 1, set 0 in vrport.ini to fully bypass)
    volatile int xrInputActions;    // 1 = create gameplay XrActions (thumbstick/trigger/buttons). 0 = pose-only legacy behaviour
    volatile int xrMonoXQueueWait;  // 1 = mono path inserts cross-queue Wait before depth capture (legacy). 0 = skip it -- avoids CP2077 async-compute Wait cycle that froze present thread.
    volatile int xrAerXQueueWait;   // 1 = AER path inserts cross-queue Wait + depth capture every present (safe vs save/load depth race). 0 = skip both (default, smooth) -- AER V2 warp falls back to NvOF-only.
    volatile int xrSnapTurnPulseMs; // duration of the discrete snap turn pulse pushed into the right stick (ms)
    volatile int xrMonoDepthCapture; // 1 = mono path runs depth capture (legacy, hangs on CP2077). 0 = skip depth capture entirely in mono mode.
    volatile int xrSnapTurnYawIndex; // which float index in deltaHead[] gets the snap yaw. Default 1.
    volatile int xrImmersiveHolsters; // 1 = visual-holster equip (default), 0 = simple slot mapping (back=Slot1, R hip=Slot2, L hip=Slot3). Published to shared[23] for the CET Holster mod.
};

static constexpr int kEnablePatchBufferTracer = 0;
static constexpr int kEnableNativeSetterTracers = 0;

static int ClampRuntimeMode(int value) {
    return value == 1 ? 1 : 0;
}

static LiveControls g_liveControls = {};

// Verbose per-frame logging (ClipCursor / depth-diag / hook spam). Off by default so
// the tester log stays readable; toggled live from the F10 Debug section. Not persisted.
volatile int g_verboseLog = 0;
static int g_launcherWidth = 2048;
static int g_launcherHeight = 2048;

static void InitRuntimePaths() {
    if (g_gameDir[0] != '\0') return;

    GetModuleFileNameA(nullptr, g_gameDir, MAX_PATH);
    char* lastSlash = strrchr(g_gameDir, '\\');
    if (lastSlash) {
        *lastSlash = '\0';
    }

    strcpy_s(g_liveControlPath, g_gameDir);
    strcat_s(g_liveControlPath, "\\vrport.ini");

    strcpy_s(g_launcherConfigPath, g_gameDir);
    strcat_s(g_launcherConfigPath, "\\vrport-launcher.ini");

    // CET sandboxes a mod's relative io.open paths to that mod's own folder, so
    // the HUD mod's '.\hud_layout.ini' resolves under its mods dir -- NOT bin\x64.
    // Write there so CyberpunkVRPort_HUD actually reads our values.
    strcpy_s(g_hudLayoutPath, g_gameDir);
    strcat_s(g_hudLayoutPath, "\\plugins\\cyber_engine_tweaks\\mods\\CyberpunkVRPort_HUD\\hud_layout.ini");

    strcpy_s(g_vrikSettingsPath, g_gameDir);
    strcat_s(g_vrikSettingsPath, "\\plugins\\cyber_engine_tweaks\\mods\\CyberpunkVRPort_VRIK\\vrik_settings.ini");
    strcpy_s(g_vrikRecenterPath, g_gameDir);
    strcat_s(g_vrikRecenterPath, "\\plugins\\cyber_engine_tweaks\\mods\\CyberpunkVRPort_VRIK\\vrik_recenter.ini");

    // Default: mouse pitch suppressed (VR uses the HMD for pitch).
    g_liveControls.xrDisableMouseY = 1;

    // Default: immersive holsters ON (current behaviour -- equip by visual holster).
    g_liveControls.xrImmersiveHolsters = 1;

    // Capture the recenter-request baseline NOW (before CET could write), so the
    // first OnGameAttached this session is seen as a change and triggers a recenter,
    // while a stale counter left over from a previous session does not.
    WIN32_FILE_ATTRIBUTE_DATA rfd;
    if (GetFileAttributesExA(g_vrikRecenterPath, GetFileExInfoStandard, &rfd)) {
        g_lastVrikRecenterWrite = rfd.ftLastWriteTime;
        FILE* rf = _fsopen(g_vrikRecenterPath, "r", _SH_DENYNO);
        if (rf) {
            char line[64]; int v = 0;
            while (fgets(line, sizeof(line), rf)) {
                if (sscanf_s(line, "recenter=%d", &v) == 1) { g_lastVrikRecenterCounter = v; break; }
            }
            fclose(rf);
        }
    }
}

static void EnsureLiveControlFileExists() {
    InitRuntimePaths();

    DWORD attrs = GetFileAttributesA(g_liveControlPath);
    if (attrs != INVALID_FILE_ATTRIBUTES) return;

    FILE* file = _fsopen(g_liveControlPath, "w", _SH_DENYNO);
    if (!file) return;

    fprintf(file, "xr_head_offset_x=0.000\n");
    fprintf(file, "xr_head_offset_y=0.000\n");
    fprintf(file, "xr_head_offset_z=0.000\n");
    fprintf(file, "xr_recenter=0\n");
    fprintf(file, "xr_mono_submit=1\n");
    fprintf(file, "xr_aer_submit=1\n");
    fprintf(file, "xr_force_fov=0\n");
    fprintf(file, "xr_menu_rect=0\n");
    fprintf(file, "xr_menu_fov=65.0\n");
    fprintf(file, "xr_3dof_movement=1\n");
    fprintf(file, "xr_dlss_matrix_hook=1\n");
    fprintf(file, "xr_dlss_slot_mode=0\n");
    fprintf(file, "xr_dlss_log_stride=600\n");
    fprintf(file, "xr_aer_pair_gate=1\n");
    fprintf(file, "xr_aer_start_eye=0\n");
    fprintf(file, "xr_aer_debug_eye=0\n");
    fprintf(file, "xr_motion_predict_ms=0.0\n");
    fprintf(file, "xr_stereo_scale=1.0\n");
    fprintf(file, "xr_world_scale=1.0\n");
    fprintf(file, "xr_ipd_scale=1.0\n");
    fprintf(file, "xr_sharpness=0.0\n");
    fprintf(file, "xr_sharpmix=1.0\n");
    fprintf(file, "xr_reuse_last_frame=0\n");
    fprintf(file, "xr_hmd_smooth=0.35\n");
    fprintf(file, "xr_hand_smooth=0.45\n");
    fprintf(file, "xr_nvof_perf=0\n");
    fprintf(file, "xr_pair_lock=0\n");
    fprintf(file, "xr_render_pose_submit=1\n");
    fprintf(file, "xr_aer_half_rate=0\n");
    fprintf(file, "xr_aer_v2=0\n");
    fprintf(file, "xr_pose_lag=1\n");
    fprintf(file, "xr_runtime=0\n");
    fprintf(file, "xr_aer_max_extrap=1.80\n");
    fprintf(file, "xr_aer_refine=0.00\n");
    fprintf(file, "xr_aer_edge_sharp=1.00\n");
    fprintf(file, "xr_aer_foveation=0.00\n");
    // Default ON: now safe via cross-queue Signal hook (CyberpunkVRPort_
    // WaitOnAllGameSignals) that GPU-Waits on every tracked game queue
    // before our depth copy. Lets the compositor do depth-aware reprojection
    // → fixes far-building shift on head turn (parallax-correct timewarp
    // instead of orientation-only). Users on broken runtimes can set 0.
    fprintf(file, "xr_depth_submit=1\n");
    fprintf(file, "xr_movement_control=0\n");
    fprintf(file, "xr_disable_mouse_y=1\n");
    fprintf(file, "xr_xinput_hook=1\n");
    fprintf(file, "xr_snap_turn=0\n");
    fprintf(file, "xr_snap_turn_angle_deg=30\n");
    fprintf(file, "xr_movement_source=0\n");
    // Default OFF for the new gameplay-input pipeline so a busted runtime
    // binding or a 14-byte XInput entry-point patch can't keep CP2077 from
    // reaching its main menu. Opt in by editing vrport.ini after confirming
    // the legacy path still launches cleanly.
    fprintf(file, "xr_xinput_install=0\n");
    fprintf(file, "xr_input_actions=0\n");
    fprintf(file, "xr_mono_xqueue_wait=0\n");
    fprintf(file, "xr_snap_turn_pulse_ms=30\n");
    fprintf(file, "xr_mono_depth_capture=0\n");
    fclose(file);
}

static void LoadLauncherConfig() {
    InitRuntimePaths();
    g_launcherWidth = 2048;
    g_launcherHeight = 2048;

    FILE* file = _fsopen(g_launcherConfigPath, "r", _SH_DENYNO);
    if (!file) return;

    char line[128];
    while (fgets(line, sizeof(line), file)) {
        int intValue = 0;
        if (sscanf_s(line, "width=%d", &intValue) == 1 ||
            sscanf_s(line, "width = %d", &intValue) == 1) {
            g_launcherWidth = intValue > 0 ? intValue : g_launcherWidth;
            continue;
        }
        if (sscanf_s(line, "height=%d", &intValue) == 1 ||
            sscanf_s(line, "height = %d", &intValue) == 1) {
            g_launcherHeight = intValue > 0 ? intValue : g_launcherHeight;
            continue;
        }
    }
    fclose(file);
}

static void SaveLauncherConfig(int width, int height) {
    InitRuntimePaths();
    g_launcherWidth = width > 0 ? width : g_launcherWidth;
    g_launcherHeight = height > 0 ? height : g_launcherHeight;

    FILE* file = _fsopen(g_launcherConfigPath, "w", _SH_DENYNO);
    if (!file) return;
    fprintf(file, "width=%d\n", g_launcherWidth);
    fprintf(file, "height=%d\n", g_launcherHeight);
    fclose(file);
}

static void WriteHudLayoutFile() {
    InitRuntimePaths();
    EnsureHudDefaults();
    FILE* file = _fsopen(g_hudLayoutPath, "w", _SH_DENYNO);
    if (!file) return;
    for (int i = 0; i < kHudFieldCount; ++i) {
        fprintf(file, "%s=%.4f\n", kHudKeys[i], g_hudValues[i]);
    }
    fclose(file);
}

static void ReadHudLayoutFile() {
    InitRuntimePaths();
    EnsureHudDefaults();
    FILE* file = _fsopen(g_hudLayoutPath, "r", _SH_DENYNO);
    if (!file) return;
    char line[160];
    while (fgets(line, sizeof(line), file)) {
        for (int i = 0; i < kHudFieldCount; ++i) {
            size_t n = strlen(kHudKeys[i]);
            // Exact key match up to '=' (so xr_hud_scale doesn't swallow xr_hud_scale_y).
            if (strncmp(line, kHudKeys[i], n) == 0 && line[n] == '=') {
                g_hudValues[i] = (float)atof(line + n + 1);
                break;
            }
        }
    }
    fclose(file);
}

// Load persisted HUD layout once, and make sure the file exists so the CET HUD
// mod has something to poll on first run.
static void EnsureHudLoaded() {
    if (g_hudLoaded) return;
    g_hudLoaded = true;
    InitRuntimePaths();
    DWORD attrs = GetFileAttributesA(g_hudLayoutPath);
    ReadHudLayoutFile();                                   // existing file -> g_hudValues
    if (attrs == INVALID_FILE_ATTRIBUTES) WriteHudLayoutFile(); // first run -> create it
}

// Publish the mouse-Y flag for the CET VRIK mod (it reads this from its own folder).
static void WriteVrikSettingsFile() {
    InitRuntimePaths();
    int v = g_liveControls.xrDisableMouseY != 0 ? 1 : 0;
    FILE* file = _fsopen(g_vrikSettingsPath, "w", _SH_DENYNO);
    if (!file) { Log("VRIK bridge: FAILED to open %s for write\n", g_vrikSettingsPath); return; }
    fprintf(file, "disable_mouse_y=%d\n", v);
    fclose(file);
    static int s_lastLogged = -1;
    if (v != s_lastLogged) { s_lastLogged = v; Log("VRIK bridge: disable_mouse_y=%d -> %s\n", v, g_vrikSettingsPath); }
}

// Poll the CET VRIK mod's recenter request (written with an incrementing counter on
// save load / OnGameAttached); recenter when the counter changes.
static void PollVrikRecenterRequest() {
    InitRuntimePaths();
    WIN32_FILE_ATTRIBUTE_DATA fd;
    if (!GetFileAttributesExA(g_vrikRecenterPath, GetFileExInfoStandard, &fd)) return;
    if (CompareFileTime(&fd.ftLastWriteTime, &g_lastVrikRecenterWrite) == 0) return;
    g_lastVrikRecenterWrite = fd.ftLastWriteTime;

    FILE* file = _fsopen(g_vrikRecenterPath, "r", _SH_DENYNO);
    if (!file) return;
    char line[64];
    int counter = -1;
    while (fgets(line, sizeof(line), file)) {
        int v = 0;
        if (sscanf_s(line, "recenter=%d", &v) == 1) { counter = v; break; }
    }
    fclose(file);
    if (counter == 0) return;
    // Baseline was captured at startup (InitRuntimePaths); any later change = a fresh
    // OnGameAttached this session.
    if (counter != g_lastVrikRecenterCounter) {
        g_lastVrikRecenterCounter = counter;
        OpenXRManager::Get().RequestRecenter();
        Log("VRIK recenter request (save load) -> recentering. counter=%d\n", counter);
    }
}

// AER V2 warp tuning accessors (atomics live in openxr_manager.cpp). The proxy
// owns their ini persistence: parse -> Set* on file change, Get* -> write on Save.
extern "C" float GetAerMaxExtrap();      extern "C" void SetAerMaxExtrap(float);
extern "C" float GetAerRefineStrength(); extern "C" void SetAerRefineStrength(float);
extern "C" float GetAerOcclusionSharp(); extern "C" void SetAerOcclusionSharp(float);
extern "C" float GetAerFoveation();      extern "C" void SetAerFoveation(float);
extern "C" float GetHmdTrackingSmooth(); extern "C" void SetHmdTrackingSmooth(float);
extern "C" float GetHandTrackingSmooth(); extern "C" void SetHandTrackingSmooth(float);

static void PollLiveControls() {
    InitRuntimePaths();
    PollVrikRecenterRequest();

    WIN32_FILE_ATTRIBUTE_DATA fileData;
    if (!GetFileAttributesExA(g_liveControlPath, GetFileExInfoStandard, &fileData)) {
        return;
    }

    if (CompareFileTime(&fileData.ftLastWriteTime, &g_lastLiveControlWrite) == 0) {
        return;
    }

    g_lastLiveControlWrite = fileData.ftLastWriteTime;

    float xrHeadOffsetX = 0.0f;
    float xrHeadOffsetY = 0.0f;
    float xrHeadOffsetZ = 0.0f;
    int xrRecenter = 0;
    int xrMonoSubmit = 1;
    int xrAERSubmit = 1;
    int xrWindowWidth = 0;
    int xrWindowHeight = 0;
    float xrForceFov = 0.0f;
    int xrMenuRect = 0;
    float xrMenuFov = 65.0f;
    float xrPitchSign = 1.0f;
    float xrPitchScale = 1.35f;
    int xrSyncSequential = 1;
    int xr3DofMovement = 1;
    int xrDLSSMatrixHook = 1;
    int xrDLSSSlotMode = 0;
    int xrDLSSLogStride = 600;
    int xrAERPairGate = 1;
    int xrAERStartEye = 0;
    int xrAERDebugEye = 0;
    int xrAERWarmupFrames = 1;
    float xrMotionPredictMs = 0.0f;
    float xrStereoScale = 1.0f;
    float xrWorldScale = 1.0f;
    float xrIpdScale = 1.0f;
    float xrSharpness = 0.0f;
    float xrSharpmix = 1.0f;
    int xrReuseLastFrame = 0;
    int xrNvofPerf = 0;
    int xrPairLock = 0;
    int xrRenderPoseSubmit = 1;
    int xrAERHalfRate = 0;
    int xrAERV2 = 0;
    int xrPoseLag = 1;
    int xrRuntime = 0;
    // Default ON: cross-queue Signal hook now serializes our depth read
    // against the game's render writers. Compositor depth-aware reprojection
    // fixes far-object shift on head turn. Users can still override via ini.
    int xrDepthSubmit = 1;
    int xrMovementControl = g_liveControls.xrMovementControl;
    int xrDisableMouseY = g_liveControls.xrDisableMouseY;
    int xrXInputHook = g_liveControls.xrXInputHook != 0 ? g_liveControls.xrXInputHook : 1;
    int xrSnapTurn = g_liveControls.xrSnapTurn;
    float xrHmdSmooth = GetHmdTrackingSmooth();
    float xrHandSmooth = GetHandTrackingSmooth();
    static const char kLegacyReuseLastFrameKey[] = {
        'x','r','_','o','u','t','p','u','t','_','r','e','a','l','v','r',0
    };
    auto tryParseIntKey = [](const char* text, const char* key, int* outValue) {
        if (!text || !key || !outValue) return false;
        const size_t keyLen = strlen(key);
        if (_strnicmp(text, key, keyLen) != 0) return false;
        const char* cursor = text + keyLen;
        while (*cursor == ' ' || *cursor == '\t') ++cursor;
        if (*cursor != '=') return false;
        ++cursor;
        while (*cursor == ' ' || *cursor == '\t') ++cursor;
        *outValue = atoi(cursor);
        return true;
    };
    float xrSnapTurnAngleDeg = g_liveControls.xrSnapTurnAngleDeg > 0.0f ? g_liveControls.xrSnapTurnAngleDeg : 30.0f;
    int xrMovementSource = g_liveControls.xrMovementSource;
    int xrXInputInstall = g_liveControls.xrXInputInstall;
    int xrInputActions = g_liveControls.xrInputActions;
    int xrMonoXQueueWait = g_liveControls.xrMonoXQueueWait;
    int xrAerXQueueWait = g_liveControls.xrAerXQueueWait;
    int xrSnapTurnPulseMs = g_liveControls.xrSnapTurnPulseMs > 0 ? g_liveControls.xrSnapTurnPulseMs : 30;
    int xrMonoDepthCapture = g_liveControls.xrMonoDepthCapture;
    int xrSnapTurnYawIndex = g_liveControls.xrSnapTurnYawIndex >= 0 && g_liveControls.xrSnapTurnYawIndex <= 3 ? g_liveControls.xrSnapTurnYawIndex : 1;
    int xrImmersiveHolsters = g_liveControls.xrImmersiveHolsters;

    FILE* file = _fsopen(g_liveControlPath, "r", _SH_DENYNO);
    if (!file) return;

    char line[128];
    while (fgets(line, sizeof(line), file)) {
        float value = 0.0f;

        if (sscanf_s(line, "xr_head_offset_x=%f", &value) == 1 ||
            sscanf_s(line, "xr_head_offset_x = %f", &value) == 1) {
            xrHeadOffsetX = value;
            continue;
        }
        if (sscanf_s(line, "xr_head_offset_y=%f", &value) == 1 ||
            sscanf_s(line, "xr_head_offset_y = %f", &value) == 1) {
            xrHeadOffsetY = value;
            continue;
        }
        if (sscanf_s(line, "xr_head_offset_z=%f", &value) == 1 ||
            sscanf_s(line, "xr_head_offset_z = %f", &value) == 1) {
            xrHeadOffsetZ = value;
            continue;
        }
        int intValue = 0;
        if (sscanf_s(line, "xr_recenter=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_recenter = %d", &intValue) == 1) {
            xrRecenter = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_mono_submit=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_mono_submit = %d", &intValue) == 1) {
            xrMonoSubmit = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_aer_submit=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_aer_submit = %d", &intValue) == 1) {
            xrAERSubmit = intValue;
            continue;
        }

        if (sscanf_s(line, "xr_window_width=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_window_width = %d", &intValue) == 1) {
            xrWindowWidth = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_window_height=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_window_height = %d", &intValue) == 1) {
            xrWindowHeight = intValue;
            continue;
        }

        if (sscanf_s(line, "xr_force_fov=%f", &value) == 1 ||
            sscanf_s(line, "xr_force_fov = %f", &value) == 1) {
            xrForceFov = value;
            continue;
        }
        if (sscanf_s(line, "xr_menu_rect=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_menu_rect = %d", &intValue) == 1) {
            xrMenuRect = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_menu_fov=%f", &value) == 1 ||
            sscanf_s(line, "xr_menu_fov = %f", &value) == 1) {
            xrMenuFov = value;
            continue;
        }
        if (sscanf_s(line, "xr_pitch_sign=%f", &value) == 1 ||
            sscanf_s(line, "xr_pitch_sign = %f", &value) == 1) {
            xrPitchSign = value < 0.0f ? -1.0f : 1.0f;
            continue;
        }
        if (sscanf_s(line, "xr_pitch_scale=%f", &value) == 1 ||
            sscanf_s(line, "xr_pitch_scale = %f", &value) == 1) {
            xrPitchScale = value > 0.01f ? value : 1.0f;
            continue;
        }
        if (sscanf_s(line, "xr_sync_sequential=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_sync_sequential = %d", &intValue) == 1) {
            xrSyncSequential = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_3dof_movement=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_3dof_movement = %d", &intValue) == 1) {
            xr3DofMovement = intValue;
            continue;
        }

        if (sscanf_s(line, "xr_dlss_matrix_hook=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_dlss_matrix_hook = %d", &intValue) == 1) {
            xrDLSSMatrixHook = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_dlss_slot_mode=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_dlss_slot_mode = %d", &intValue) == 1) {
            xrDLSSSlotMode = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_dlss_log_stride=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_dlss_log_stride = %d", &intValue) == 1) {
            xrDLSSLogStride = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_aer_pair_gate=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_aer_pair_gate = %d", &intValue) == 1) {
            xrAERPairGate = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_aer_start_eye=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_aer_start_eye = %d", &intValue) == 1) {
            xrAERStartEye = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_aer_debug_eye=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_aer_debug_eye = %d", &intValue) == 1) {
            xrAERDebugEye = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_aer_warmup_frames=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_aer_warmup_frames = %d", &intValue) == 1) {
            xrAERWarmupFrames = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_motion_predict_ms=%f", &value) == 1 ||
            sscanf_s(line, "xr_motion_predict_ms = %f", &value) == 1) {
            xrMotionPredictMs = value;
            continue;
        }
        if (sscanf_s(line, "xr_stereo_scale=%f", &value) == 1 ||
            sscanf_s(line, "xr_stereo_scale = %f", &value) == 1) {
            xrStereoScale = value;
            continue;
        }
        if (sscanf_s(line, "xr_world_scale=%f", &value) == 1 ||
            sscanf_s(line, "xr_world_scale = %f", &value) == 1) {
            xrWorldScale = value;
            continue;
        }
        if (sscanf_s(line, "xr_ipd_scale=%f", &value) == 1 ||
            sscanf_s(line, "xr_ipd_scale = %f", &value) == 1) {
            xrIpdScale = value;
            continue;
        }
        if (sscanf_s(line, "xr_sharpness=%f", &value) == 1 ||
            sscanf_s(line, "xr_sharpness = %f", &value) == 1) {
            xrSharpness = value;
            continue;
        }
        if (sscanf_s(line, "xr_sharpmix=%f", &value) == 1 ||
            sscanf_s(line, "xr_sharpmix = %f", &value) == 1) {
            xrSharpmix = value;
            continue;
        }
        if (sscanf_s(line, "xr_aer_max_extrap=%f", &value) == 1 ||
            sscanf_s(line, "xr_aer_max_extrap = %f", &value) == 1) {
            SetAerMaxExtrap(value);
            continue;
        }
        if (sscanf_s(line, "xr_aer_refine=%f", &value) == 1 ||
            sscanf_s(line, "xr_aer_refine = %f", &value) == 1) {
            SetAerRefineStrength(value);
            continue;
        }
        if (sscanf_s(line, "xr_aer_edge_sharp=%f", &value) == 1 ||
            sscanf_s(line, "xr_aer_edge_sharp = %f", &value) == 1) {
            SetAerOcclusionSharp(value);
            continue;
        }
        if (sscanf_s(line, "xr_aer_foveation=%f", &value) == 1 ||
            sscanf_s(line, "xr_aer_foveation = %f", &value) == 1) {
            SetAerFoveation(value);
            continue;
        }
        if (sscanf_s(line, "xr_hmd_smooth=%f", &value) == 1 ||
            sscanf_s(line, "xr_hmd_smooth = %f", &value) == 1) {
            xrHmdSmooth = value;
            continue;
        }
        if (sscanf_s(line, "xr_hand_smooth=%f", &value) == 1 ||
            sscanf_s(line, "xr_hand_smooth = %f", &value) == 1) {
            xrHandSmooth = value;
            continue;
        }
        if (sscanf_s(line, "xr_render_pose_submit=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_render_pose_submit = %d", &intValue) == 1) {
            xrRenderPoseSubmit = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_aer_half_rate=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_aer_half_rate = %d", &intValue) == 1) {
            xrAERHalfRate = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_aer_v2=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_aer_v2 = %d", &intValue) == 1) {
            xrAERV2 = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_reuse_last_frame=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_reuse_last_frame = %d", &intValue) == 1 ||
            tryParseIntKey(line, kLegacyReuseLastFrameKey, &intValue)) {
            xrReuseLastFrame = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_nvof_perf=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_nvof_perf = %d", &intValue) == 1) {
            xrNvofPerf = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_pair_lock=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_pair_lock = %d", &intValue) == 1) {
            xrPairLock = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_pose_lag=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_pose_lag = %d", &intValue) == 1) {
            xrPoseLag = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_runtime=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_runtime = %d", &intValue) == 1) {
            xrRuntime = ClampRuntimeMode(intValue);
            continue;
        }
        if (sscanf_s(line, "xr_depth_submit=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_depth_submit = %d", &intValue) == 1) {
            xrDepthSubmit = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_movement_control=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_movement_control = %d", &intValue) == 1) {
            xrMovementControl = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_disable_mouse_y=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_disable_mouse_y = %d", &intValue) == 1) {
            xrDisableMouseY = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_xinput_hook=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_xinput_hook = %d", &intValue) == 1) {
            xrXInputHook = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_snap_turn=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_snap_turn = %d", &intValue) == 1) {
            xrSnapTurn = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_snap_turn_angle_deg=%f", &value) == 1 ||
            sscanf_s(line, "xr_snap_turn_angle_deg = %f", &value) == 1) {
            xrSnapTurnAngleDeg = value;
            continue;
        }
        if (sscanf_s(line, "xr_movement_source=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_movement_source = %d", &intValue) == 1) {
            xrMovementSource = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_xinput_install=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_xinput_install = %d", &intValue) == 1) {
            xrXInputInstall = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_input_actions=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_input_actions = %d", &intValue) == 1) {
            xrInputActions = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_mono_xqueue_wait=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_mono_xqueue_wait = %d", &intValue) == 1) {
            xrMonoXQueueWait = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_aer_xqueue_wait=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_aer_xqueue_wait = %d", &intValue) == 1) {
            xrAerXQueueWait = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_snap_turn_pulse_ms=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_snap_turn_pulse_ms = %d", &intValue) == 1) {
            xrSnapTurnPulseMs = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_mono_depth_capture=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_mono_depth_capture = %d", &intValue) == 1) {
            xrMonoDepthCapture = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_snap_turn_yaw_index=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_snap_turn_yaw_index = %d", &intValue) == 1) {
            xrSnapTurnYawIndex = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_immersive_holsters=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_immersive_holsters = %d", &intValue) == 1) {
            xrImmersiveHolsters = intValue;
            continue;
        }

    }
    fclose(file);

    const int prevXrRecenter = g_liveControls.xrRecenter;
    const int prevXrMonoSubmit = g_liveControls.xrMonoSubmit;
    const int prevXrAERSubmit = g_liveControls.xrAERSubmit;
    const int prevXrAERStartEye = g_liveControls.xrAERStartEye;
    const bool changed = g_liveControls.xrHeadOffsetX != xrHeadOffsetX ||
        g_liveControls.xrHeadOffsetY != xrHeadOffsetY ||
        g_liveControls.xrHeadOffsetZ != xrHeadOffsetZ ||
        g_liveControls.xrRecenter != xrRecenter ||
        g_liveControls.xrMonoSubmit != xrMonoSubmit ||
        g_liveControls.xrAERSubmit != xrAERSubmit ||
        g_liveControls.xrForceFov != xrForceFov ||
        g_liveControls.xrMenuRect != xrMenuRect ||
        g_liveControls.xrMenuFov != xrMenuFov ||
        g_liveControls.xr3DofMovement != xr3DofMovement ||
        g_liveControls.xrDLSSMatrixHook != xrDLSSMatrixHook ||
        g_liveControls.xrDLSSSlotMode != xrDLSSSlotMode ||
        g_liveControls.xrDLSSLogStride != xrDLSSLogStride ||
        g_liveControls.xrAERPairGate != xrAERPairGate ||
        g_liveControls.xrAERStartEye != xrAERStartEye ||
        g_liveControls.xrAERDebugEye != xrAERDebugEye ||
        g_liveControls.xrMotionPredictMs != xrMotionPredictMs ||
        g_liveControls.xrStereoScale != xrStereoScale ||
        g_liveControls.xrWorldScale != xrWorldScale ||
        g_liveControls.xrIpdScale != xrIpdScale ||
        g_liveControls.xrSharpness != xrSharpness ||
        g_liveControls.xrSharpmix != xrSharpmix ||
        g_liveControls.xrReuseLastFrame != xrReuseLastFrame ||
        g_liveControls.xrNvofPerf != xrNvofPerf ||
        g_liveControls.xrPairLock != xrPairLock ||
        g_liveControls.xrRenderPoseSubmit != xrRenderPoseSubmit ||
        g_liveControls.xrAERHalfRate != xrAERHalfRate ||
        g_liveControls.xrAERV2 != xrAERV2 ||
        g_liveControls.xrRuntime != xrRuntime ||
        g_liveControls.xrDepthSubmit != xrDepthSubmit;

    g_liveControls.xrHeadOffsetX = xrHeadOffsetX;
    g_liveControls.xrHeadOffsetY = xrHeadOffsetY;
    g_liveControls.xrHeadOffsetZ = xrHeadOffsetZ;
    g_liveControls.xrRecenter = xrRecenter;
    g_liveControls.xrMonoSubmit = xrMonoSubmit;
    g_liveControls.xrAERSubmit = xrAERSubmit;
    g_liveControls.xrForceFov = xrForceFov;
    g_liveControls.xrMenuRect = xrMenuRect;
    g_liveControls.xrMenuFov = xrMenuFov;
    g_liveControls.xr3DofMovement = xr3DofMovement;
    g_liveControls.xrDLSSMatrixHook = xrDLSSMatrixHook;
    g_liveControls.xrDLSSSlotMode = xrDLSSSlotMode;
    g_liveControls.xrDLSSLogStride = xrDLSSLogStride > 0 ? xrDLSSLogStride : 0;
    g_liveControls.xrAERPairGate = xrAERPairGate;
    g_liveControls.xrAERStartEye = xrAERStartEye != 0 ? 1 : 0;
    g_liveControls.xrAERDebugEye = xrAERDebugEye;
    g_liveControls.xrMotionPredictMs = xrMotionPredictMs >= 0.0f ? xrMotionPredictMs : 0.0f;
    g_liveControls.xrStereoScale = xrStereoScale < 0.0f ? 0.0f : (xrStereoScale > 10.0f ? 10.0f : xrStereoScale);
    g_liveControls.xrWorldScale = xrWorldScale < 0.05f ? 0.05f : (xrWorldScale > 20.0f ? 20.0f : xrWorldScale);
    g_liveControls.xrIpdScale = xrIpdScale < 0.0f ? 0.0f : (xrIpdScale > 5.0f ? 5.0f : xrIpdScale);
    g_liveControls.xrSharpness = xrSharpness < 0.0f ? 0.0f : (xrSharpness > 1.0f ? 1.0f : xrSharpness);
    g_liveControls.xrSharpmix = xrSharpmix < 0.0f ? 0.0f : (xrSharpmix > 1.0f ? 1.0f : xrSharpmix);
    g_liveControls.xrReuseLastFrame = xrReuseLastFrame != 0 ? 1 : 0;
    g_liveControls.xrNvofPerf = xrNvofPerf < 0 ? 0 : (xrNvofPerf > 2 ? 2 : xrNvofPerf);
    g_liveControls.xrPairLock = xrPairLock != 0 ? 1 : 0;
    g_liveControls.xrRenderPoseSubmit = xrRenderPoseSubmit != 0 ? 1 : 0;
    g_liveControls.xrAERHalfRate = xrAERHalfRate != 0 ? 1 : 0;
    g_liveControls.xrAERV2 = xrAERV2 != 0 ? 1 : 0;
    g_liveControls.xrPoseLag = xrPoseLag;
    g_liveControls.xrRuntime = ClampRuntimeMode(xrRuntime);
    g_liveControls.xrDepthSubmit = xrDepthSubmit != 0 ? 1 : 0;
    // xrMovementSource is the authoritative locomotion mode (0..3); legacy
    // xrMovementControl mirrors it for old configs (0 = Game, anything else
    // means VR-driven so map to legacy 1).
    if (xrMovementSource < 0 || xrMovementSource > 3) xrMovementSource = xrMovementControl != 0 ? 1 : 0;
    g_liveControls.xrMovementSource = xrMovementSource;
    g_liveControls.xrMovementControl = xrMovementSource != 0 ? 1 : 0;
    g_liveControls.xrDisableMouseY = xrDisableMouseY != 0 ? 1 : 0;
    g_liveControls.xrXInputHook = xrXInputHook != 0 ? 1 : 0;
    g_liveControls.xrSnapTurn = xrSnapTurn != 0 ? 1 : 0;
    g_liveControls.xrSnapTurnAngleDeg = xrSnapTurnAngleDeg > 0.0f ? xrSnapTurnAngleDeg : 30.0f;
    g_liveControls.xrXInputInstall = xrXInputInstall != 0 ? 1 : 0;
    g_liveControls.xrInputActions = xrInputActions != 0 ? 1 : 0;
    g_liveControls.xrMonoXQueueWait = xrMonoXQueueWait != 0 ? 1 : 0;
    g_liveControls.xrAerXQueueWait = xrAerXQueueWait != 0 ? 1 : 0;
    g_liveControls.xrSnapTurnPulseMs = xrSnapTurnPulseMs > 0 ? xrSnapTurnPulseMs : 30;
    g_liveControls.xrMonoDepthCapture = xrMonoDepthCapture != 0 ? 1 : 0;
    g_liveControls.xrSnapTurnYawIndex = (xrSnapTurnYawIndex >= 0 && xrSnapTurnYawIndex <= 3) ? xrSnapTurnYawIndex : 1;
    g_liveControls.xrImmersiveHolsters = xrImmersiveHolsters != 0 ? 1 : 0;
    OpenXRManager::Get().SetImmersiveHolsters(g_liveControls.xrImmersiveHolsters);
    SetHmdTrackingSmooth(xrHmdSmooth);
    SetHandTrackingSmooth(xrHandSmooth);
    WriteVrikSettingsFile(); // keep the CET-facing bridge file in sync with vrport.ini
    if (prevXrRecenter == 0 && xrRecenter != 0) {
        OpenXRManager::Get().RequestRecenter();
        Log("OpenXR recenter requested.\n");
    }

    if (prevXrMonoSubmit != xrMonoSubmit) {
        OpenXRManager::Get().SetMonoSubmitEnabled(xrMonoSubmit != 0);
        Log("OpenXR mono submit %s.\n", xrMonoSubmit != 0 ? "enabled" : "disabled");
    }

    if (prevXrAERSubmit != xrAERSubmit || prevXrAERStartEye != g_liveControls.xrAERStartEye) {
        OpenXRManager::Get().SetAERSubmitEnabled(xrAERSubmit != 0);
        Log("OpenXR AER submit %s. startEye=%d\n", xrAERSubmit != 0 ? "enabled" : "disabled", g_liveControls.xrAERStartEye);
    }

    if (changed && g_verboseLog) {
        Log("Live controls updated: xr_head_offset=(%.4f,%.4f,%.4f) xr_recenter=%d xr_mono_submit=%d xr_aer_submit=%d xr_force_fov=%.3f xr_menu_rect=%d xr_menu_fov=%.3f xr_3dof_movement=%d xr_dlss_matrix_hook=%d xr_dlss_slot_mode=%d xr_dlss_log_stride=%d xr_aer_pair_gate=%d xr_aer_start_eye=%d xr_aer_debug_eye=%d xr_motion_predict_ms=%.2f xr_stereo_scale=%.3f xr_render_pose_submit=%d xr_aer_half_rate=%d xr_aer_v2=%d xr_runtime=%d\n",
            g_liveControls.xrHeadOffsetX, g_liveControls.xrHeadOffsetY, g_liveControls.xrHeadOffsetZ, g_liveControls.xrRecenter, g_liveControls.xrMonoSubmit, g_liveControls.xrAERSubmit, g_liveControls.xrForceFov, g_liveControls.xrMenuRect, g_liveControls.xrMenuFov, g_liveControls.xr3DofMovement, g_liveControls.xrDLSSMatrixHook, g_liveControls.xrDLSSSlotMode, g_liveControls.xrDLSSLogStride, g_liveControls.xrAERPairGate, g_liveControls.xrAERStartEye, g_liveControls.xrAERDebugEye, g_liveControls.xrMotionPredictMs, g_liveControls.xrStereoScale, g_liveControls.xrRenderPoseSubmit, g_liveControls.xrAERHalfRate, g_liveControls.xrAERV2, g_liveControls.xrRuntime);
        if (g_liveControls.xrRuntime != 0) {
            Log("Live controls: xr_runtime=%d will apply on next startup before OpenXR init.\n", g_liveControls.xrRuntime);
        }
    }
}

static LiveControlsUiState MakeLiveControlsUiState() {
    LiveControlsUiState state{};
    state.xrHeadOffsetX = g_liveControls.xrHeadOffsetX;
    state.xrHeadOffsetY = g_liveControls.xrHeadOffsetY;
    state.xrHeadOffsetZ = g_liveControls.xrHeadOffsetZ;
    state.xrRecenter = g_liveControls.xrRecenter;
    state.xrMonoSubmit = g_liveControls.xrMonoSubmit;
    state.xrAERSubmit = g_liveControls.xrAERSubmit;
    state.xrForceFov = g_liveControls.xrForceFov;
    state.xrMenuRect = g_liveControls.xrMenuRect;
    state.xrMenuFov = g_liveControls.xrMenuFov;
    state.xr3DofMovement = g_liveControls.xr3DofMovement;
    state.xrDLSSMatrixHook = g_liveControls.xrDLSSMatrixHook;
    state.xrDLSSSlotMode = g_liveControls.xrDLSSSlotMode;
    state.xrDLSSLogStride = g_liveControls.xrDLSSLogStride;
    state.xrAERPairGate = g_liveControls.xrAERPairGate;
    state.xrAERStartEye = g_liveControls.xrAERStartEye;
    state.xrAERDebugEye = g_liveControls.xrAERDebugEye;
    state.xrMotionPredictMs = g_liveControls.xrMotionPredictMs;
    state.xrStereoScale = g_liveControls.xrStereoScale;
    state.xrWorldScale = g_liveControls.xrWorldScale;
    state.xrIpdScale = g_liveControls.xrIpdScale;
    state.xrSharpness = g_liveControls.xrSharpness;
    state.xrSharpmix = g_liveControls.xrSharpmix;
    state.xrReuseLastFrame = g_liveControls.xrReuseLastFrame;
    state.xrNvofPerf = g_liveControls.xrNvofPerf;
    state.xrPairLock = g_liveControls.xrPairLock;
    state.xrRenderPoseSubmit = g_liveControls.xrRenderPoseSubmit;
    state.xrAERHalfRate = g_liveControls.xrAERHalfRate;
    state.xrAERV2 = g_liveControls.xrAERV2;
    state.xrPoseLag = g_liveControls.xrPoseLag;
    state.xrRuntime = g_liveControls.xrRuntime;
    state.xrMovementControl = g_liveControls.xrMovementControl;
    state.xrDisableMouseY = g_liveControls.xrDisableMouseY;
    state.xrXInputHook = g_liveControls.xrXInputHook;
    state.xrSnapTurn = g_liveControls.xrSnapTurn;
    state.xrSnapTurnAngleDeg = g_liveControls.xrSnapTurnAngleDeg;
    state.xrMovementSource = g_liveControls.xrMovementSource;
    state.xrXInputInstall = g_liveControls.xrXInputInstall;
    state.xrInputActions = g_liveControls.xrInputActions;
    state.xrMonoXQueueWait = g_liveControls.xrMonoXQueueWait;
    state.xrAerXQueueWait = g_liveControls.xrAerXQueueWait;
    state.xrMonoDepthCapture = g_liveControls.xrMonoDepthCapture;
    state.xrSnapTurnPulseMs = g_liveControls.xrSnapTurnPulseMs;
    state.xrImmersiveHolsters = g_liveControls.xrImmersiveHolsters;
    // HUD placement isn't stored in g_liveControls; pull the last overlay-set
    // values (loaded from hud_layout.ini) into the contiguous xrHud* block.
    EnsureHudLoaded();
    memcpy(&state.xrHudScale, g_hudValues, kHudFieldCount * sizeof(float));
    return state;
}

static void PersistLiveControlsUiState(const LiveControlsUiState& state) {
    InitRuntimePaths();
    FILE* file = _fsopen(g_liveControlPath, "w", _SH_DENYNO);
    if (!file) return;

    fprintf(file, "xr_head_offset_x=%.4f\n", state.xrHeadOffsetX);
    fprintf(file, "xr_head_offset_y=%.4f\n", state.xrHeadOffsetY);
    fprintf(file, "xr_head_offset_z=%.4f\n", state.xrHeadOffsetZ);
    fprintf(file, "xr_recenter=0\n");
    fprintf(file, "xr_mono_submit=%d\n", state.xrMonoSubmit != 0 ? 1 : 0);
    fprintf(file, "xr_aer_submit=%d\n", state.xrAERSubmit != 0 ? 1 : 0);
    fprintf(file, "xr_force_fov=%.3f\n", state.xrForceFov);
    fprintf(file, "xr_menu_rect=%d\n", state.xrMenuRect != 0 ? 1 : 0);
    fprintf(file, "xr_menu_fov=%.3f\n", state.xrMenuFov);
    fprintf(file, "xr_3dof_movement=%d\n", state.xr3DofMovement != 0 ? 1 : 0);
    fprintf(file, "xr_dlss_matrix_hook=%d\n", state.xrDLSSMatrixHook != 0 ? 1 : 0);
    fprintf(file, "xr_dlss_slot_mode=%d\n", state.xrDLSSSlotMode);
    fprintf(file, "xr_dlss_log_stride=%d\n", state.xrDLSSLogStride);
    fprintf(file, "xr_aer_pair_gate=%d\n", state.xrAERPairGate != 0 ? 1 : 0);
    fprintf(file, "xr_aer_start_eye=%d\n", state.xrAERStartEye != 0 ? 1 : 0);
    fprintf(file, "xr_aer_debug_eye=%d\n", state.xrAERDebugEye < 0 ? 0 : (state.xrAERDebugEye > 3 ? 3 : state.xrAERDebugEye));
    fprintf(file, "xr_motion_predict_ms=%.2f\n", state.xrMotionPredictMs);
    fprintf(file, "xr_stereo_scale=%.3f\n", state.xrStereoScale);
    fprintf(file, "xr_world_scale=%.3f\n", state.xrWorldScale);
    fprintf(file, "xr_ipd_scale=%.3f\n", state.xrIpdScale);
    fprintf(file, "xr_sharpness=%.3f\n", state.xrSharpness);
    fprintf(file, "xr_sharpmix=%.3f\n", state.xrSharpmix);
    fprintf(file, "xr_reuse_last_frame=%d\n", state.xrReuseLastFrame != 0 ? 1 : 0);
    fprintf(file, "xr_nvof_perf=%d\n", state.xrNvofPerf);
    fprintf(file, "xr_pair_lock=%d\n", state.xrPairLock != 0 ? 1 : 0);
    fprintf(file, "xr_render_pose_submit=%d\n", state.xrRenderPoseSubmit != 0 ? 1 : 0);
    fprintf(file, "xr_aer_half_rate=%d\n", state.xrAERHalfRate != 0 ? 1 : 0);
    fprintf(file, "xr_aer_v2=%d\n", state.xrAERV2 != 0 ? 1 : 0);
    fprintf(file, "xr_pose_lag=%d\n", state.xrPoseLag);
    fprintf(file, "xr_runtime=%d\n", ClampRuntimeMode(state.xrRuntime));
    fprintf(file, "xr_aer_max_extrap=%.3f\n", GetAerMaxExtrap());
    fprintf(file, "xr_aer_refine=%.3f\n", GetAerRefineStrength());
    fprintf(file, "xr_aer_edge_sharp=%.3f\n", GetAerOcclusionSharp());
    fprintf(file, "xr_aer_foveation=%.3f\n", GetAerFoveation());
    fprintf(file, "xr_hmd_smooth=%.3f\n", GetHmdTrackingSmooth());
    fprintf(file, "xr_hand_smooth=%.3f\n", GetHandTrackingSmooth());
    fprintf(file, "xr_movement_control=%d\n", state.xrMovementControl != 0 ? 1 : 0);
    fprintf(file, "xr_disable_mouse_y=%d\n", state.xrDisableMouseY != 0 ? 1 : 0);
    fprintf(file, "xr_xinput_hook=%d\n", state.xrXInputHook != 0 ? 1 : 0);
    fprintf(file, "xr_snap_turn=%d\n", state.xrSnapTurn != 0 ? 1 : 0);
    fprintf(file, "xr_snap_turn_angle_deg=%.2f\n", state.xrSnapTurnAngleDeg > 0.0f ? state.xrSnapTurnAngleDeg : 30.0f);
    fprintf(file, "xr_movement_source=%d\n", state.xrMovementSource < 0 ? 0 : (state.xrMovementSource > 3 ? 3 : state.xrMovementSource));
    fprintf(file, "xr_xinput_install=%d\n", state.xrXInputInstall != 0 ? 1 : 0);
    fprintf(file, "xr_input_actions=%d\n", state.xrInputActions != 0 ? 1 : 0);
    fprintf(file, "xr_mono_xqueue_wait=%d\n", state.xrMonoXQueueWait != 0 ? 1 : 0);
    fprintf(file, "xr_aer_xqueue_wait=%d\n", state.xrAerXQueueWait != 0 ? 1 : 0);
    fprintf(file, "xr_mono_depth_capture=%d\n", state.xrMonoDepthCapture != 0 ? 1 : 0);
    fprintf(file, "xr_snap_turn_pulse_ms=%d\n", state.xrSnapTurnPulseMs > 0 ? state.xrSnapTurnPulseMs : 30);
    fprintf(file, "xr_immersive_holsters=%d\n", state.xrImmersiveHolsters != 0 ? 1 : 0);
    fclose(file);

    WIN32_FILE_ATTRIBUTE_DATA fileData;
    if (GetFileAttributesExA(g_liveControlPath, GetFileExInfoStandard, &fileData)) {
        g_lastLiveControlWrite = fileData.ftLastWriteTime;
    }
}

extern "C" void GetLiveControlsUiState(LiveControlsUiState* outState) {
    if (!outState) return;
    *outState = MakeLiveControlsUiState();
}

extern "C" void RequestLiveControlsRecenter() {
    g_liveControls.xrRecenter = 0;
    OpenXRManager::Get().RequestRecenter();
    Log("ImGui: OpenXR recenter requested.\n");
}

extern "C" void SetLiveControlsUiState(const LiveControlsUiState* state, int persistToFile) {
    if (!state) return;

    const int prevMono = g_liveControls.xrMonoSubmit;
    const int prevAER = g_liveControls.xrAERSubmit;
    const int prevAERStartEye = g_liveControls.xrAERStartEye;

    g_liveControls.xrHeadOffsetX = state->xrHeadOffsetX;
    g_liveControls.xrHeadOffsetY = state->xrHeadOffsetY;
    g_liveControls.xrHeadOffsetZ = state->xrHeadOffsetZ;
    g_liveControls.xrRecenter = 0;
    g_liveControls.xrMonoSubmit = state->xrMonoSubmit != 0 ? 1 : 0;
    g_liveControls.xrAERSubmit = state->xrAERSubmit != 0 ? 1 : 0;
    g_liveControls.xrForceFov = state->xrForceFov > 0.0f ? state->xrForceFov : 0.0f;
    g_liveControls.xrMenuRect = state->xrMenuRect != 0 ? 1 : 0;
    g_liveControls.xrMenuFov = state->xrMenuFov > 1.0f ? state->xrMenuFov : 65.0f;
    g_liveControls.xr3DofMovement = state->xr3DofMovement != 0 ? 1 : 0;
    g_liveControls.xrDLSSMatrixHook = state->xrDLSSMatrixHook != 0 ? 1 : 0;
    g_liveControls.xrDLSSSlotMode = state->xrDLSSSlotMode;
    g_liveControls.xrDLSSLogStride = state->xrDLSSLogStride > 0 ? state->xrDLSSLogStride : 0;
    g_liveControls.xrAERPairGate = state->xrAERPairGate != 0 ? 1 : 0;
    g_liveControls.xrAERStartEye = state->xrAERStartEye != 0 ? 1 : 0;
    g_liveControls.xrAERDebugEye = state->xrAERDebugEye < 0 ? 0 : (state->xrAERDebugEye > 3 ? 3 : state->xrAERDebugEye);
    g_liveControls.xrMotionPredictMs = state->xrMotionPredictMs >= 0.0f ? state->xrMotionPredictMs : 0.0f;
    g_liveControls.xrStereoScale = state->xrStereoScale < 0.0f ? 0.0f : (state->xrStereoScale > 10.0f ? 10.0f : state->xrStereoScale);
    g_liveControls.xrWorldScale = state->xrWorldScale < 0.05f ? 0.05f : (state->xrWorldScale > 20.0f ? 20.0f : state->xrWorldScale);
    g_liveControls.xrIpdScale = state->xrIpdScale < 0.0f ? 0.0f : (state->xrIpdScale > 5.0f ? 5.0f : state->xrIpdScale);
    g_liveControls.xrSharpness = state->xrSharpness < 0.0f ? 0.0f : (state->xrSharpness > 1.0f ? 1.0f : state->xrSharpness);
    g_liveControls.xrSharpmix = state->xrSharpmix < 0.0f ? 0.0f : (state->xrSharpmix > 1.0f ? 1.0f : state->xrSharpmix);
    g_liveControls.xrReuseLastFrame = state->xrReuseLastFrame != 0 ? 1 : 0;
    g_liveControls.xrNvofPerf = state->xrNvofPerf < 0 ? 0 : (state->xrNvofPerf > 2 ? 2 : state->xrNvofPerf);
    g_liveControls.xrPairLock = state->xrPairLock != 0 ? 1 : 0;
    g_liveControls.xrRenderPoseSubmit = state->xrRenderPoseSubmit != 0 ? 1 : 0;
    g_liveControls.xrAERHalfRate = state->xrAERHalfRate != 0 ? 1 : 0;
    g_liveControls.xrAERV2 = state->xrAERV2 != 0 ? 1 : 0;
    g_liveControls.xrPoseLag = state->xrPoseLag;
    g_liveControls.xrRuntime = ClampRuntimeMode(state->xrRuntime);
    {
        int src = state->xrMovementSource;
        if (src < 0 || src > 3) src = state->xrMovementControl != 0 ? 1 : 0;
        g_liveControls.xrMovementSource = src;
        g_liveControls.xrMovementControl = src != 0 ? 1 : 0;
    }
    g_liveControls.xrDisableMouseY = state->xrDisableMouseY != 0 ? 1 : 0;
    g_liveControls.xrXInputHook = state->xrXInputHook != 0 ? 1 : 0;
    g_liveControls.xrSnapTurn = state->xrSnapTurn != 0 ? 1 : 0;
    g_liveControls.xrSnapTurnAngleDeg = state->xrSnapTurnAngleDeg > 0.0f ? state->xrSnapTurnAngleDeg : 30.0f;
    g_liveControls.xrXInputInstall = state->xrXInputInstall != 0 ? 1 : 0;
    g_liveControls.xrInputActions = state->xrInputActions != 0 ? 1 : 0;
    g_liveControls.xrMonoXQueueWait = state->xrMonoXQueueWait != 0 ? 1 : 0;
    g_liveControls.xrAerXQueueWait = state->xrAerXQueueWait != 0 ? 1 : 0;
    g_liveControls.xrMonoDepthCapture = state->xrMonoDepthCapture != 0 ? 1 : 0;
    g_liveControls.xrSnapTurnPulseMs = state->xrSnapTurnPulseMs > 0 ? state->xrSnapTurnPulseMs : 30;
    g_liveControls.xrImmersiveHolsters = state->xrImmersiveHolsters != 0 ? 1 : 0;
    OpenXRManager::Get().SetImmersiveHolsters(g_liveControls.xrImmersiveHolsters);
    WriteVrikSettingsFile(); // publish mouse-Y flag for the CET VRIK mod

    if (prevMono != g_liveControls.xrMonoSubmit) {
        OpenXRManager::Get().SetMonoSubmitEnabled(g_liveControls.xrMonoSubmit != 0);
        Log("ImGui: OpenXR mono submit %s.\n", g_liveControls.xrMonoSubmit != 0 ? "enabled" : "disabled");
    }
    if (prevAER != g_liveControls.xrAERSubmit || prevAERStartEye != g_liveControls.xrAERStartEye) {
        OpenXRManager::Get().SetAERSubmitEnabled(g_liveControls.xrAERSubmit != 0);
        Log("ImGui: OpenXR AER submit %s. startEye=%d\n", g_liveControls.xrAERSubmit != 0 ? "enabled" : "disabled", g_liveControls.xrAERStartEye);
    }
    if (state->xrRecenter != 0) {
        RequestLiveControlsRecenter();
    }

    // HUD placement: store the overlay's values and publish hud_layout.ini, which
    // the CET HUD mod polls. Done every call (not just on persist) so dragging a
    // slider updates the HUD live.
    EnsureHudLoaded();
    memcpy(g_hudValues, &state->xrHudScale, kHudFieldCount * sizeof(float));
    WriteHudLayoutFile();

    if (persistToFile != 0) {
        PersistLiveControlsUiState(MakeLiveControlsUiState());
    }
}

static void PollHotkeys() {
    static bool f7WasDown = false;
    static bool f8WasDown = false;

    const bool f7Down = (GetAsyncKeyState(VK_F7) & 0x8000) != 0;
    const bool f8Down = (GetAsyncKeyState(VK_F8) & 0x8000) != 0;

    if (f7Down && !f7WasDown) {
        OpenXRManager::Get().RequestRecenter();
        Log("Hotkey F7: OpenXR recenter requested.\n");
    }

    if (f8Down && !f8WasDown) {
        g_liveControls.xrMenuRect = g_liveControls.xrMenuRect != 0 ? 0 : 1;
        Log("Hotkey F8: xr_menu_rect=%d (%s, AER unchanged).\n",
            g_liveControls.xrMenuRect,
            g_liveControls.xrMenuRect != 0 ? "small HMD rectangle" : "full HMD rectangle");
    }

    f7WasDown = f7Down;
    f8WasDown = f8Down;
}

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

extern "C" void SetWindowResolutionAndPersist(int width, int height) {
    SaveLauncherConfig(width, height);
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

static UINT GetForcedRenderWidthValue() {
    uint32_t w = 0, h = 0;
    if (OpenXRManager::Get().GetRecommendedRenderTargetSize(&w, &h) && w > 0) {
        return w;
    }
    return 0;
}

static UINT GetForcedRenderHeightValue() {
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

static UINT GetForcedSquareResolutionValue() {
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

extern "C" float GetForcedFov() {
    return g_liveControls.xrForceFov;
}

extern "C" float GetMenuFov() {
    return g_liveControls.xrMenuFov;
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

extern "C" int GetAERPairGate() {
    return g_liveControls.xrAERPairGate;
}

extern "C" int GetAERStartEye() {
    return g_liveControls.xrAERStartEye;
}

extern "C" int GetAERDebugEye() {
    return g_liveControls.xrAERDebugEye;
}

extern "C" int GetAERWarmupFrames() {
    return 0;
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

extern "C" int GetAERHalfRate() {
    return g_liveControls.xrAERHalfRate;
}

extern "C" int GetAERV2Enabled() {
    return g_liveControls.xrAERV2;
}

extern "C" float GetVrSharpness() {
    return g_liveControls.xrSharpness;
}

extern "C" float GetVrSharpmix() {
    return g_liveControls.xrSharpmix;
}

extern "C" int GetReuseLastFrameOutput() {
    const char* e = getenv("CPVR_REUSE_LAST_FRAME");
    if (!(e && *e)) {
        e = getenv("CPVR_OUTPUT_REALVR");
    }
    if (e && *e) return atoi(e) != 0 ? 1 : 0;
    return g_liveControls.xrReuseLastFrame;
}

extern "C" int GetVrNvofPerf() {
    const char* e = getenv("CPVR_NVOF_PERF");
    if (e && *e) { int v = atoi(e); return v < 0 ? 0 : (v > 2 ? 2 : v); }
    return g_liveControls.xrNvofPerf;
}

extern "C" int GetVrPairLock() {
    const char* e = getenv("CPVR_PAIRLOCK");
    if (e && *e) return atoi(e) != 0 ? 1 : 0;
    return g_liveControls.xrPairLock;
}

extern "C" int GetXrRuntimeMode() {
    return g_liveControls.xrRuntime;
}

extern "C" int GetInputActionsEnabled() {
    return g_liveControls.xrInputActions != 0 ? 1 : 0;
}

extern "C" int GetMonoXQueueWait() {
    return g_liveControls.xrMonoXQueueWait != 0 ? 1 : 0;
}

extern "C" int GetAerXQueueWait() {
    return g_liveControls.xrAerXQueueWait != 0 ? 1 : 0;
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


void Log(const char* fmt, ...) {
    if (!g_logFile) {
        char logPath[MAX_PATH];
        GetModuleFileNameA(nullptr, logPath, MAX_PATH);
        char* lastSlash = strrchr(logPath, '\\');
        if (lastSlash) *(lastSlash + 1) = 0;
        strcat_s(logPath, "cyberpunkvrport.log");
        g_logFile = _fsopen(logPath, "w", _SH_DENYNO);
    }
    if (!g_logFile) return;

    va_list args;
    va_start(args, fmt);
    vfprintf(g_logFile, fmt, args);
    va_end(args);
    fflush(g_logFile);
}

static uint8_t* g_arenaBase = nullptr;
static size_t g_arenaOffset = 0;

void* AllocateTrampoline(void* targetAddress, size_t size) {
    if (!g_arenaBase) {
        SYSTEM_INFO sysInfo;
        GetSystemInfo(&sysInfo);

        uintptr_t target = reinterpret_cast<uintptr_t>(targetAddress);
        uintptr_t minAddr = target > 0x7FFFFFFF ? target - 0x7FFFFFFF : 0;
        uintptr_t maxAddr = target + 0x7FFFFFFF;
        if (maxAddr < target) maxAddr = UINTPTR_MAX;
        minAddr -= minAddr % sysInfo.dwAllocationGranularity;

        for (uintptr_t addr = target - sysInfo.dwAllocationGranularity; addr > minAddr; addr -= sysInfo.dwAllocationGranularity) {
            g_arenaBase = static_cast<uint8_t*>(VirtualAlloc(reinterpret_cast<void*>(addr), 65536, MEM_COMMIT | MEM_RESERVE, PAGE_EXECUTE_READWRITE));
            if (g_arenaBase) break;
        }
        if (!g_arenaBase) {
            for (uintptr_t addr = target + sysInfo.dwAllocationGranularity; addr < maxAddr; addr += sysInfo.dwAllocationGranularity) {
                g_arenaBase = static_cast<uint8_t*>(VirtualAlloc(reinterpret_cast<void*>(addr), 65536, MEM_COMMIT | MEM_RESERVE, PAGE_EXECUTE_READWRITE));
                if (g_arenaBase) break;
            }
        }
    }

    if (g_arenaBase && g_arenaOffset + size <= 65536) {
        void* ret = g_arenaBase + g_arenaOffset;
        g_arenaOffset += size;
        return ret;
    }
    return nullptr;
}

static void WriteRipDisp(uint8_t* code, int dispPos, const volatile void* dst) {
    uintptr_t dstAddr = reinterpret_cast<uintptr_t>(dst);
    uintptr_t nextInstruction = reinterpret_cast<uintptr_t>(code + dispPos + 4);
    *reinterpret_cast<int32_t*>(code + dispPos) = static_cast<int32_t>(dstAddr - nextInstruction);
}

static void WriteMovRaxImm64(uint8_t* code, int& pos, uintptr_t value) {
    code[pos++] = 0x48;
    code[pos++] = 0xB8;
    *reinterpret_cast<uint64_t*>(code + pos) = static_cast<uint64_t>(value);
    pos += 8;
}

static void WriteMovR11Imm64(uint8_t* code, int& pos, uintptr_t value) {
    code[pos++] = 0x49;
    code[pos++] = 0xBB;
    *reinterpret_cast<uint64_t*>(code + pos) = static_cast<uint64_t>(value);
    pos += 8;
}

static bool ReadFloatSafe(uintptr_t addr, float* out) {
    __try {
        *out = *reinterpret_cast<const float*>(addr);
        return true;
    }
    __except (EXCEPTION_EXECUTE_HANDLER) {
        return false;
    }
}

static bool WriteFloatSafe(uintptr_t addr, float value) {
    __try {
        *reinterpret_cast<float*>(addr) = value;
        return true;
    }
    __except (EXCEPTION_EXECUTE_HANDLER) {
        return false;
    }
}

static bool ReadFloatArraySafe(const float* src, float* out, size_t count) {
    if (!src || !out) return false;
    for (size_t i = 0; i < count; ++i) {
        if (!ReadFloatSafe(reinterpret_cast<uintptr_t>(src + i), &out[i])) {
            return false;
        }
    }
    return true;
}

static bool WriteFloatArraySafe(float* dst, const float* values, size_t count) {
    if (!dst || !values) return false;
    for (size_t i = 0; i < count; ++i) {
        if (!WriteFloatSafe(reinterpret_cast<uintptr_t>(dst + i), values[i])) {
            return false;
        }
    }
    return true;
}

static bool LooksProjectionLike(const float* values, size_t count) {
    if (!values || count < 16) return false;

    const float m00 = values[0];
    const float m11 = values[5];
    const float m03 = values[3];
    const float m13 = values[7];
    const float m23 = values[11];
    const float m33 = values[15];

    if (!(m00 > 0.2f && m00 < 8.0f && m11 > 0.2f && m11 < 8.0f)) return false;
    if (fabsf(m03) > 0.1f || fabsf(m13) > 0.1f) return false;
    if (!(fabsf(m23) > 0.1f || fabsf(m33) < 0.1f || fabsf(m33 - 1.0f) < 0.1f)) return false;
    return true;
}

static void LogMatrix4x4(const char* prefix, const float* values) {
    if (!prefix || !values) return;
    Log("%s\n", prefix);
    Log("  [%.6f %.6f %.6f %.6f]\n", values[0], values[1], values[2], values[3]);
    Log("  [%.6f %.6f %.6f %.6f]\n", values[4], values[5], values[6], values[7]);
    Log("  [%.6f %.6f %.6f %.6f]\n", values[8], values[9], values[10], values[11]);
    Log("  [%.6f %.6f %.6f %.6f]\n", values[12], values[13], values[14], values[15]);
}

static bool ReadU8Safe(uintptr_t addr, uint8_t* out) {
    __try {
        *out = *reinterpret_cast<const uint8_t*>(addr);
        return true;
    }
    __except (EXCEPTION_EXECUTE_HANDLER) {
        return false;
    }
}

static bool ReadU32Safe(uintptr_t addr, uint32_t* out) {
    __try {
        *out = *reinterpret_cast<const uint32_t*>(addr);
        return true;
    }
    __except (EXCEPTION_EXECUTE_HANDLER) {
        return false;
    }
}

static bool WriteU32Safe(uintptr_t addr, uint32_t value) {
    __try {
        *reinterpret_cast<uint32_t*>(addr) = value;
        return true;
    }
    __except (EXCEPTION_EXECUTE_HANDLER) {
        return false;
    }
}

static bool ReadPtrSafe(uintptr_t addr, uintptr_t* out) {
    __try {
        *out = *reinterpret_cast<const uintptr_t*>(addr);
        return true;
    }
    __except (EXCEPTION_EXECUTE_HANDLER) {
        return false;
    }
}

static void InitGameModuleInfo() {
    if (g_gameModuleBase != 0 && g_gameModuleSize != 0) return;

    HMODULE gameModule = GetModuleHandleA("Cyberpunk2077.exe");
    if (!gameModule) return;

    MODULEINFO moduleInfo = {};
    if (!GetModuleInformation(GetCurrentProcess(), gameModule, &moduleInfo, sizeof(moduleInfo))) {
        return;
    }

    g_gameModuleBase = reinterpret_cast<uintptr_t>(moduleInfo.lpBaseOfDll);
    g_gameModuleSize = static_cast<size_t>(moduleInfo.SizeOfImage);
}

static bool IsInGameModule(uintptr_t addr) {
    if (g_gameModuleBase == 0 || g_gameModuleSize == 0) return false;
    return addr >= g_gameModuleBase && addr < (g_gameModuleBase + g_gameModuleSize);
}

static bool IsReadableAddressRange(uintptr_t addr, size_t size) {
    if (!addr || size == 0) return false;
    if (addr < 0x10000000000ULL) return false; // reject small/tagged values like 0x0000000100000000

    MEMORY_BASIC_INFORMATION mbi = {};
    if (VirtualQuery(reinterpret_cast<const void*>(addr), &mbi, sizeof(mbi)) != sizeof(mbi)) {
        return false;
    }
    if (mbi.State != MEM_COMMIT) return false;
    if ((mbi.Protect & PAGE_GUARD) != 0 || (mbi.Protect & PAGE_NOACCESS) != 0) return false;

    const DWORD readableMask = PAGE_READONLY | PAGE_READWRITE | PAGE_WRITECOPY |
        PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY;
    if ((mbi.Protect & readableMask) == 0) return false;

    uintptr_t regionEnd = reinterpret_cast<uintptr_t>(mbi.BaseAddress) + mbi.RegionSize;
    if (regionEnd < addr) return false;
    return addr + size <= regionEnd;
}

static void LogVec4At(const char* label, uintptr_t addr) {
    if (!addr) {
        Log("  %s: null\n", label);
        return;
    }

    float v[4] = {};
    for (int i = 0; i < 4; ++i) {
        if (!ReadFloatSafe(addr + i * sizeof(float), &v[i])) {
            Log("  %s @%p unreadable\n", label, reinterpret_cast<void*>(addr));
            return;
        }
    }

    Log("  %s @%p = (%.6f, %.6f, %.6f, %.6f)\n",
        label, reinterpret_cast<void*>(addr), v[0], v[1], v[2], v[3]);
}

static void LogFloatAt(const char* label, uintptr_t addr) {
    float value = 0.0f;
    if (!addr) {
        Log("  %s: null\n", label);
        return;
    }
    if (!ReadFloatSafe(addr, &value)) {
        Log("  %s @%p unreadable\n", label, reinterpret_cast<void*>(addr));
        return;
    }
    Log("  %s @%p = %.6f\n", label, reinterpret_cast<void*>(addr), value);
}

static void LogU32FloatAt(const char* label, uintptr_t addr) {
    uint32_t u32Value = 0;
    float f32Value = 0.0f;
    if (!addr) {
        Log("  %s: null\n", label);
        return;
    }
    if (!ReadU32Safe(addr, &u32Value) || !ReadFloatSafe(addr, &f32Value)) {
        Log("  %s @%p unreadable\n", label, reinterpret_cast<void*>(addr));
        return;
    }
    Log("  %s @%p = u32=%u (0x%08X) f32=%.6f\n",
        label,
        reinterpret_cast<void*>(addr),
        u32Value,
        u32Value,
        f32Value);
}

static void LogU8At(const char* label, uintptr_t addr) {
    uint8_t value = 0;
    if (!addr) {
        Log("  %s: null\n", label);
        return;
    }
    if (!ReadU8Safe(addr, &value)) {
        Log("  %s @%p unreadable\n", label, reinterpret_cast<void*>(addr));
        return;
    }
    Log("  %s @%p = 0x%02X (%u)\n", label, reinterpret_cast<void*>(addr), value, value);
}

static void LogPtrAt(const char* label, uintptr_t addr) {
    uintptr_t value = 0;
    if (!addr) {
        Log("  %s: null\n", label);
        return;
    }
    if (!ReadPtrSafe(addr, &value)) {
        Log("  %s @%p unreadable\n", label, reinterpret_cast<void*>(addr));
        return;
    }
    Log("  %s @%p = %p\n", label, reinterpret_cast<void*>(addr), reinterpret_cast<void*>(value));
}

static void LogPtrPayloadVec4At(const char* label, uintptr_t addr) {
    uintptr_t value = 0;
    if (!addr) {
        Log("  %s: null\n", label);
        return;
    }
    if (!ReadPtrSafe(addr, &value)) {
        Log("  %s @%p unreadable\n", label, reinterpret_cast<void*>(addr));
        return;
    }
    Log("  %s @%p = %p\n", label, reinterpret_cast<void*>(addr), reinterpret_cast<void*>(value));
    if (value != 0 && IsReadableAddressRange(value, sizeof(float) * 4)) {
        char payloadLabel[96];
        sprintf_s(payloadLabel, "%s payload", label);
        LogVec4At(payloadLabel, value);
    } else if (value != 0) {
        Log("  %s payload @%p skipped (not a plausible readable pointer)\n",
            label, reinterpret_cast<void*>(value));
    }
}

static void LogStackWindowAt(const char* label, uintptr_t rsp, int slots) {
    if (!rsp) {
        Log("  %s: null\n", label);
        return;
    }

    Log("  %s @%p\n", label, reinterpret_cast<void*>(rsp));
    for (int i = 0; i < slots; ++i) {
        uintptr_t entryAddr = rsp + static_cast<uintptr_t>(i) * sizeof(uintptr_t);
        uintptr_t value = 0;
        if (!ReadPtrSafe(entryAddr, &value)) {
            Log("    [%02d] @%p unreadable\n", i, reinterpret_cast<void*>(entryAddr));
            return;
        }

        if (IsInGameModule(value)) {
            Log("    [%02d] @%p = %p (game+rva 0x%llX)\n",
                i,
                reinterpret_cast<void*>(entryAddr),
                reinterpret_cast<void*>(value),
                static_cast<unsigned long long>(value - g_gameModuleBase));
        } else {
            Log("    [%02d] @%p = %p\n",
                i,
                reinterpret_cast<void*>(entryAddr),
                reinterpret_cast<void*>(value));
        }
    }
}

// ======================== TELEMETRY ========================

struct TelemetryData {
    volatile uint32_t locateHits;
    volatile uint32_t _pad1;
    volatile uintptr_t locateRbx;
    volatile float locateXmm0;
    volatile uint32_t _pad2[3];

    volatile uint32_t patchHits;
    volatile uint32_t _pad3;
    volatile uintptr_t patchRdx;
    volatile uintptr_t patchRsi;
    volatile float patchXmm0[4];

    volatile uint32_t finalHits;
    volatile uint32_t _pad4;
    volatile uintptr_t finalRsi;

    volatile uint32_t deltaHeadHits;
    volatile uint32_t _pad5;
    volatile uintptr_t deltaHeadRcx;
    volatile float deltaHeadXmm0;
    volatile uint32_t _pad6[3];

    volatile uint32_t moveXYHits;
    volatile uint32_t _pad7;
    volatile uintptr_t moveXYRsi;
    volatile float moveXYXmm0;
    volatile uint32_t _pad8[3];

    volatile uint32_t freeDeltaHits;
    volatile uint32_t _pad9;
    volatile uintptr_t freeDeltaRsi;
    volatile float freeDeltaXmm3;
    volatile uint32_t _pad10[3];
};
static TelemetryData* g_telemetry = nullptr;

static constexpr int kLocateTelemetryOffset = static_cast<int>(offsetof(TelemetryData, locateHits));
static constexpr int kPatchTelemetryOffset = static_cast<int>(offsetof(TelemetryData, patchHits));
static constexpr int kFinalTelemetryOffset = static_cast<int>(offsetof(TelemetryData, finalHits));
static constexpr int kDeltaHeadTelemetryOffset = static_cast<int>(offsetof(TelemetryData, deltaHeadHits));
static constexpr int kMoveXYTelemetryOffset = static_cast<int>(offsetof(TelemetryData, moveXYHits));
static constexpr int kFreeDeltaTelemetryOffset = static_cast<int>(offsetof(TelemetryData, freeDeltaHits));

struct SetterTraceData {
    volatile uint32_t metaWriteHits;
    volatile uint32_t _pad1;
    volatile uintptr_t metaWriteTemp;
    volatile uintptr_t metaWriteMeta;
    volatile uintptr_t metaWriteRsp;

    volatile uint32_t metaConsumeHits;
    volatile uint32_t _pad2;
    volatile uintptr_t metaConsumeTemp;
    volatile uintptr_t metaConsumeMeta;
    volatile uintptr_t metaConsumeRsp;

    volatile uint32_t clearHits;
    volatile uint32_t _pad3;
    volatile uintptr_t clearTemp;
    volatile uintptr_t clearReturn;
};
static SetterTraceData* g_setterTrace = nullptr;

static constexpr int kMetaWriteTraceOffset = static_cast<int>(offsetof(SetterTraceData, metaWriteHits));
static constexpr int kMetaConsumeTraceOffset = static_cast<int>(offsetof(SetterTraceData, metaConsumeHits));
static constexpr int kClearTraceOffset = static_cast<int>(offsetof(SetterTraceData, clearHits));

static volatile uintptr_t g_settingsResPtr = 0;
static volatile uintptr_t g_dlssResPtr = 0;
static uint64_t g_settingsResHits = 0;
static uint64_t g_dlssResHits = 0;
static volatile uint64_t g_dlssMatricesHits = 0;
static volatile uintptr_t g_dlssMatricesThis = 0;
static volatile uintptr_t g_dlssMatricesState = 0;
static volatile uint32_t g_dlssMatricesSlot = 0;
static volatile uint32_t g_dlssMatricesAdjustedSlot = 0;
static volatile int g_dlssMatricesEye = -1;
static uintptr_t g_dlssMatricesHookSite = 0;
static uintptr_t g_dlssMatricesCallTarget = 0;

static float* GetShotShared();  // shared-mem accessor (defined below)

// MAP PIN-DRIFT FIX. The map pins slide off the background on pan/zoom because
// the game's UI projection assumes 16:9 but we force a 1:1 square resolution.
// While the world
// map is open (shared[81], set by redscript bridge SetVRMenuOpen), STOP applying
// our square-resolution override — let the game use its real 16:9 resolution for
// the map's UI projection so pins track the background correctly.
static void ApplySettingsResolutionOverride(uintptr_t settingsPtr) {
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

static void ApplyDLSSResolutionOverride(uintptr_t dlssPtr) {
    const UINT forcedSquare = GetForcedSquareResolutionValue();
    if (!dlssPtr || forcedSquare == 0) {
        return;
    }

    // World map open? Suspend the override (same gate as Settings — see above).
    {
        uint32_t mapFlag = 0;
        if (float* sh = GetShotShared()) {
            mapFlag = reinterpret_cast<volatile uint32_t*>(sh)[81];
        }
        if (mapFlag != 0u) {
            return;
        }
    }

    WriteU32Safe(dlssPtr + 0x04, forcedSquare);
    WriteU32Safe(dlssPtr + 0x18, forcedSquare);
    WriteU32Safe(dlssPtr + 0x1C, forcedSquare);
}

static void ApplyKnownResolutionOverrides() {
    const uintptr_t settingsPtr = g_settingsResPtr;
    const uintptr_t dlssPtr = g_dlssResPtr;
    if (settingsPtr != 0) {
        ApplySettingsResolutionOverride(settingsPtr);
    }
    if (dlssPtr != 0) {
        ApplyDLSSResolutionOverride(dlssPtr);
    }
}

static volatile float g_pitchOverrideValue = 0.0f;
static volatile float g_normalFovOverrideValue = 0.0f;
static volatile float g_lodFovOverride = 120.0f;
static bool g_pitchHookInstalled = false;
static bool g_normalFovHookInstalled = false;
static bool g_forceHeadingUpdateHookInstalled = false;
static volatile int g_menuModeValue = 0;

static bool ExtractOpenXRYawPitch(const OpenXRHeadPose& xrPose, float* outYaw, float* outPitch) {
    if (!xrPose.valid) {
        return false;
    }

    const float qx = xrPose.oriX;
    const float qy = xrPose.oriY;
    const float qz = xrPose.oriZ;
    const float qw = xrPose.oriW;

    // Forward vector (0, 0, -1) rotated by the headset pose in OpenXR space.
    const float fx = -2.0f * (qx * qz + qy * qw);
    const float fy = 2.0f * (qx * qw - qy * qz);
    const float fz = 2.0f * (qx * qx + qy * qy) - 1.0f;

    if (outYaw) {
        *outYaw = atan2f(-fx, -fz);
    }
    if (outPitch) {
        const float clampedFy = fy < -1.0f ? -1.0f : (fy > 1.0f ? 1.0f : fy);
        *outPitch = asinf(clampedFy);
    }
    return true;
}

static float GetDesiredGameHorizontalFov() {
    XrFovf left{}, right{};
    if (OpenXRManager::Get().GetCurrentEyeFov(0, &left) &&
        OpenXRManager::Get().GetCurrentEyeFov(1, &right)) {
        const RuntimeFovCorrection corr = ComputeRuntimeFovCorrection(left, right);
        const float fov = GetCorrectedGameHorizontalFovDeg(corr);
        return fov > 1.0f ? fov : 0.0f;
    }
    const float runtimeFov = OpenXRManager::Get().GetRuntimeHorizontalFovDeg();
    return runtimeFov > 1.0f ? runtimeFov : 0.0f;
}

static float GetWorldScale() {
    // Uniform world scale. Multiplies both the eye separation and the head-
    // translation gain, so lowering it makes the world appear bigger.
    const float ws = g_liveControls.xrWorldScale;
    return (ws > 0.0f) ? ws : 1.0f;
}

static float GetDesiredHalfIpd() {
    // Auto per-person/per-headset: the half-IPD comes straight from the OpenXR
    // runtime view separation, so it already adapts to whoever is wearing the HMD.
    const float runtimeIpd = OpenXRManager::Get().GetRuntimeIpd();
    const float halfIpd = runtimeIpd > 0.001f ? runtimeIpd * 0.5f : 0.032f;
    // Eye-separation = runtime half-IPD x ipdScale x worldScale x stereoScale.
    // The neutral baseline is ipdScale=1.0 (raw runtime IPD, typically
    // +-0.033 m). The old 1.5 value exaggerated depth
    // and distorted perceived world scale even when the camera FOV was correct.
    // Keep xr_stereo_scale as an optional taste multiplier, but default the core
    // IPD path to 1:1 with the runtime.
    float ipdScale = g_liveControls.xrIpdScale;
    if (!(ipdScale > 0.0f)) {
        ipdScale = 1.0f;  // guard zero-init window / bad values (honest runtime IPD)
    }
    float stereoScale = g_liveControls.xrStereoScale;
    if (!(stereoScale > 0.0f)) {
        stereoScale = 1.0f;  // guard zero-init window / bad values
    }
    return halfIpd > 0.0001f ? halfIpd * GetWorldScale() * ipdScale * stereoScale : 0.0f;
}

static bool IsFiniteFloat(float value) {
    return std::isfinite(value);
}

static bool IsPlausibleUnitVector3(const float* v) {
    if (!v) return false;
    if (!IsFiniteFloat(v[0]) || !IsFiniteFloat(v[1]) || !IsFiniteFloat(v[2])) return false;

    const float lenSq = v[0] * v[0] + v[1] * v[1] + v[2] * v[2];
    return lenSq > 0.25f && lenSq < 4.0f;
}

static bool IsPlausibleUnitQuaternion(const float* q) {
    if (!q) return false;
    if (!IsFiniteFloat(q[0]) || !IsFiniteFloat(q[1]) || !IsFiniteFloat(q[2]) || !IsFiniteFloat(q[3])) return false;

    const float lenSq = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
    return lenSq > 0.25f && lenSq < 4.0f;
}

static bool IsPlausiblePositionVec4(const float* v) {
    if (!v) return false;
    if (!IsFiniteFloat(v[0]) || !IsFiniteFloat(v[1]) || !IsFiniteFloat(v[2]) || !IsFiniteFloat(v[3])) return false;
    return fabsf(v[3] - 1.0f) < 0.25f;
}

static void ComputeRightVectorFromQuaternion(const float* q, float* outRight) {
    if (!q || !outRight) return;

    const float x = q[0];
    const float y = q[1];
    const float z = q[2];
    const float w = q[3];

    outRight[0] = 1.0f - 2.0f * (y * y + z * z);
    outRight[1] = 2.0f * (x * y + z * w);
    outRight[2] = 2.0f * (x * z - y * w);
}

static void BuildGameViewRowsFromQuaternion(const float* q, float* outViewRows) {
    if (!q || !outViewRows) return;

    const float x = q[0];
    const float y = q[1];
    const float z = q[2];
    const float w = q[3];

    outViewRows[0] = 1.0f - 2.0f * (y * y + z * z);
    outViewRows[1] = 2.0f * (x * y + z * w);
    outViewRows[2] = 2.0f * (x * z - y * w);
    outViewRows[3] = 0.0f;

    outViewRows[4] = 2.0f * (x * z + y * w);
    outViewRows[5] = 2.0f * (y * z - x * w);
    outViewRows[6] = 1.0f - 2.0f * (x * x + y * y);
    outViewRows[7] = 0.0f;

    outViewRows[8] = 2.0f * (x * y - z * w);
    outViewRows[9] = 1.0f - 2.0f * (x * x + z * z);
    outViewRows[10] = 2.0f * (y * z + x * w);
    outViewRows[11] = 0.0f;
}

static void ApplyFinalCameraOrientationFromQuat(float* rsiPtr, const float* q) {
    if (!rsiPtr || !q || !IsPlausibleUnitQuaternion(q)) return;

    float viewRows[12] = {};
    BuildGameViewRowsFromQuaternion(q, viewRows);

    // Keep the raw quaternion in sync so any downstream camera rebuilds see VR orientation.
    WriteFloatArraySafe(rsiPtr + 4, q, 4);

    float cameraMtx[16] = {};
    if (ReadFloatArraySafe(rsiPtr + 20, cameraMtx, 16)) {
        cameraMtx[0] = viewRows[0];
        cameraMtx[1] = viewRows[4];
        cameraMtx[2] = viewRows[8];
        cameraMtx[4] = viewRows[1];
        cameraMtx[5] = viewRows[5];
        cameraMtx[6] = viewRows[9];
        cameraMtx[8] = viewRows[2];
        cameraMtx[9] = viewRows[6];
        cameraMtx[10] = viewRows[10];
        WriteFloatArraySafe(rsiPtr + 20, cameraMtx, 16);
    }

    WriteFloatArraySafe(rsiPtr + 68, viewRows, 12);

    float viewPacket[16] = {};
    if (ReadFloatArraySafe(rsiPtr + 204, viewPacket, 16)) {
        float scale0 = sqrtf(viewPacket[0] * viewPacket[0] + viewPacket[1] * viewPacket[1] + viewPacket[2] * viewPacket[2]);
        float scale1 = sqrtf(viewPacket[4] * viewPacket[4] + viewPacket[5] * viewPacket[5] + viewPacket[6] * viewPacket[6]);
        if (!IsFiniteFloat(scale0) || scale0 < 0.05f || scale0 > 10.0f) scale0 = 1.0f;
        if (!IsFiniteFloat(scale1) || scale1 < 0.05f || scale1 > 10.0f) scale1 = scale0;

        viewPacket[0] = viewRows[0] * scale0;
        viewPacket[1] = viewRows[1] * scale0;
        viewPacket[2] = viewRows[2] * scale0;
        viewPacket[4] = viewRows[4] * scale1;
        viewPacket[5] = viewRows[5] * scale1;
        viewPacket[6] = viewRows[6] * scale1;
        viewPacket[12] = viewRows[8];
        viewPacket[13] = viewRows[9];
        viewPacket[14] = viewRows[10];

        const int32_t* finalPosFP = reinterpret_cast<const int32_t*>(rsiPtr);
        const float posScale25 = 25.0f / 65536.0f;
        viewPacket[8] = static_cast<float>(finalPosFP[0]) * posScale25;
        viewPacket[9] = static_cast<float>(finalPosFP[1]) * posScale25;
        viewPacket[10] = static_cast<float>(finalPosFP[2]) * posScale25;

        WriteFloatArraySafe(rsiPtr + 204, viewPacket, 16);
    }
}

static bool IsPlausibleCameraSpan(const float* a, const float* b) {
    if (!a || !b) return false;
    if (!IsPlausiblePositionVec4(a) || !IsPlausiblePositionVec4(b)) return false;

    const float dx = b[0] - a[0];
    const float dy = b[1] - a[1];
    const float dz = b[2] - a[2];
    const float spanSq = dx * dx + dy * dy + dz * dz;
    return spanSq < 25.0f;
}

static volatile int32_t g_lastLocatePosFP[3] = {};
// LATE IPD SHIFT: the per-eye stereo offset, computed (and eye-signed) in
// LocateCamera but NOT applied to the located camera there. The located camera
// stays at the head CENTER so the engine's IK/physics/VRIK see a stable,
// non-jittering head. OnFinalCameraCallback adds this shift to the final render
// camera only — post-IK, just before projection.
static volatile int32_t g_lastIpdShiftFP[3] = {};
volatile float g_lastLocateQuat[4] = { 0.0f, 0.0f, 0.0f, 1.0f };  // located (HMD-injected) game-world cam quat; read by the overlay barrel crosshair
static volatile uint32_t g_lastLocateSeq = 0;
static volatile uint32_t g_renderedSeq = 0;
static volatile uint8_t g_locateEyeBySeq[256] = {};
static volatile int g_renderedEye = 0;

extern "C" uint32_t GetRenderedCameraSeq() {
    return g_renderedSeq;
}

extern "C" int GetRenderedCameraEye() {
    return g_renderedEye;
}

extern "C" int GetMenuMode() {
    return g_menuModeValue;
}

static void NormalizeQuat(float& x, float& y, float& z, float& w) {
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

static void MulQuat(float ax, float ay, float az, float aw,
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
// Layout (float slots in "CyberpunkVR_Hands_Shared", 128 floats; [0..19] hands, [33..48]
// calib are taken): [50] valid-seq, [51]/[52] locatedCamPtr lo/hi, [53..56] controllerAimQuat.
static float* g_shotShared = nullptr;
static HANDLE g_shotSharedHandle = nullptr;
static float* GetShotShared() {
    if (!g_shotShared) {
        g_shotSharedHandle = OpenFileMappingA(FILE_MAP_ALL_ACCESS, FALSE, "CyberpunkVR_Hands_Shared");
        if (!g_shotSharedHandle)
            g_shotSharedHandle = CreateFileMappingA(INVALID_HANDLE_VALUE, NULL, PAGE_READWRITE, 0, 512, "CyberpunkVR_Hands_Shared");
        if (g_shotSharedHandle)
            g_shotShared = static_cast<float*>(MapViewOfFile(g_shotSharedHandle, FILE_MAP_ALL_ACCESS, 0, 0, 512));
    }
    return g_shotShared;
}

static uint64_t g_locateCameraHits = 0;
extern "C" void __fastcall OnLocateCameraCallback(float* rbxPtr, float xmm0_val) {
    (void)xmm0_val;
    g_locateCameraHits++;
    if (g_telemetry) {
        g_telemetry->locateHits = static_cast<uint32_t>(g_locateCameraHits);
        g_telemetry->locateRbx = reinterpret_cast<uint64_t>(rbxPtr);
        g_telemetry->locateXmm0 = xmm0_val;
    }
    if (!rbxPtr || reinterpret_cast<uintptr_t>(rbxPtr) < 0x10000) return;

    int32_t* posFP = reinterpret_cast<int32_t*>(rbxPtr);
    float* quat = reinterpret_cast<float*>(rbxPtr + 4); // +16 bytes = +4 floats

    float dummy;
    if (!ReadFloatSafe(reinterpret_cast<uintptr_t>(quat), &dummy)) return;

    float qx = quat[0];
    float qy = quat[1];
    float qz = quat[2];
    float qw = quat[3];

    // SKIP-HMD test (decoupled-aim experiment): the plugin publishes a shot-frame flag
    // [57] and a master mode [58] to shared mem. mode 1 = always skip the HMD orientation
    // overwrite (view follows the game's stick/mouse aim, no head); mode 2 = skip only on
    // the shot frame (let the engine's native snap-to-aim through -> bullet should follow
    // AIM not the head). When skipping, we leave the game's camera quat untouched.
    bool skipHmdOrientation = false;
    if (float* sh = GetShotShared()) {
        const uint32_t mode = reinterpret_cast<volatile uint32_t*>(sh)[58];
        const uint32_t shotFrame = reinterpret_cast<volatile uint32_t*>(sh)[57];
        if (mode == 1u) skipHmdOrientation = true;
        else if (mode == 2u && shotFrame != 0u) skipHmdOrientation = true;
    }
    // Menu stability: in a full-screen menu (e.g. the world map),
    // do NOT drive the game camera with the HMD orientation, otherwise the menu/
    // map SWIMS as you turn your head. Leave the game camera quat untouched so the
    // menu view stays put. Detection: the native menu-mode hook OR the redscript
    // world-map bridge flag (shared slot [81]) for menus the native hook misses.
    bool menuOpen = (g_menuModeValue != 0);
    if (!menuOpen) {
        if (float* sh = GetShotShared()) {
            if (reinterpret_cast<volatile uint32_t*>(sh)[81] != 0u) menuOpen = true;
        }
    }
    if (menuOpen) skipHmdOrientation = true;

    const float baseQx = qx;
    const float baseQy = qy;
    const float baseQz = qz;
    const float baseQw = qw;

    // In this camera path the game-local basis is effectively:
    // X = right, Y = forward, Z = up.
    // The standard quaternion basis formulas assume X = right, Y = up, Z = forward,
    // so the produced "up" vector is the game's forward, and the produced "forward"
    // vector is the game's up.
    const float bodyGameForwardX = 2.0f * (baseQx * baseQy - baseQz * baseQw);
    const float bodyGameForwardY = 1.0f - 2.0f * (baseQx * baseQx + baseQz * baseQz);

    // POSE PAIR LOCKING: fetch the render eye FIRST, then take a pair-locked head
    // pose — eye0 samples live + freezes, eye1 replays eye0's pose. Both eyes of
    // the stereo pair therefore drive the camera (and below, VRIK) from ONE head
    // pose, so the engine's IK/skeleton stops rebuilding between the ~11 ms-apart
    // left/right renders (the body/hands jitter seen even on the flat mirror).
    OpenXRHeadPose xrPose{};
    const int renderEye = OpenXRManager::Get().GetCurrentRenderEyeIndex();
    // POSE PAIR LOCKING: in AER, READ the frozen snapshot the engine ALREADY built
    // this pair's skeleton from (published in OnPresent at the pair boundary, before
    // the animation pass). LocateCamera runs DURING render, AFTER animation, so it
    // must NOT re-sample — the camera view must match the body the plugin already
    // posed. In mono there is no pairing, so sample live (no added latency).
    // CPVR_PAIRLOCK=0 disables the pose-pair-lock: sample the LIVE head pose every
    // camera-locate instead of the per-pair frozen snapshot. Disabling it trades
    // pair-consistent body alignment for a small per-eye skeleton tear.
    // vrport.ini xr_pair_lock (0 = live pose every locate; env CPVR_PAIRLOCK overrides).
    const bool pairLockOn = GetVrPairLock() != 0;
    const bool aerActiveForPose = OpenXRManager::Get().IsAERSubmitEnabled() && pairLockOn;
    const bool hasXR = aerActiveForPose
        ? OpenXRManager::Get().GetPairLockedHeadPose(&xrPose)
        : OpenXRManager::Get().GetHeadPose(&xrPose);
    if (hasXR) {
        uint32_t currentSeq = g_lastLocateSeq + 1;
        g_locateEyeBySeq[currentSeq % 256] = static_cast<uint8_t>(renderEye & 1);
        if (float* sh = GetShotShared()) {
            // [94] current render eye for CET/Lua, [95] desired half IPD.
            sh[94] = static_cast<float>(renderEye);
            sh[95] = GetDesiredHalfIpd();
        }
        OpenXRManager::Get().StoreRenderEyePose(0, xrPose, currentSeq);
        OpenXRManager::Get().StoreRenderEyePose(1, xrPose, currentSeq);

        // NOTE: shared-memory hands/head ([0..19],[89],[90]) are NO LONGER flushed
        // here. The VRIK plugin reads them during the engine's ANIMATION pass, which
        // runs BEFORE this render hook — flushing here landed one stage too late and
        // tore the skeleton across the eye pair. They are now published in OnPresent
        // at the pair boundary (UpdatePairLock + FlushHandsToShared), before the next
        // pair's animation.

        // Keep mouse/controller yaw as the body heading, but do not add mouse-Y pitch
        // on top of HMD pitch. The headset supplies vertical look in VR.
        const float gameYaw = atan2f(-bodyGameForwardX, bodyGameForwardY);

        const float cy = cosf(gameYaw * 0.5f);
        const float sy = sinf(gameYaw * 0.5f);

        const float xrGameX = xrPose.oriX;
        const float xrGameY = -xrPose.oriZ;
        const float xrGameZ = xrPose.oriY;
        const float xrGameW = xrPose.oriW;

        // Apply game yaw, then VR headset rotation. Mouse-Y pitch is intentionally ignored.
        float tmpX, tmpY, tmpZ, tmpW;
        MulQuat(0.0f, 0.0f, sy, cy, xrGameX, xrGameY, xrGameZ, xrGameW, tmpX, tmpY, tmpZ, tmpW);

        float outX = tmpX;
        float outY = tmpY;
        float outZ = tmpZ;
        float outW = tmpW;

        // Runtime-frustum asymmetry compensation.
        // The game camera path is scalar-FOV based, while runtimes often report
        // slightly asymmetric per-eye frusta. Recenter those frusta with a
        // quarter-angle correction and apply matching pitch/yaw quaternions so the
        // visible image stays 1:1 with the runtime.
        XrFovf leftFov{}, rightFov{};
        if (OpenXRManager::Get().GetCurrentEyeFov(0, &leftFov) &&
            OpenXRManager::Get().GetCurrentEyeFov(1, &rightFov)) {
            const RuntimeFovCorrection corr = ComputeRuntimeFovCorrection(leftFov, rightFov);
            if (corr.pitchEnabled || corr.yawEnabled) {
                float cX = outX, cY = outY, cZ = outZ, cW = outW;
                if (corr.pitchEnabled) {
                    const float hp = corr.pitchDeltaRad * 0.5f;
                    float tX, tY, tZ, tW;
                    MulQuat(cX, cY, cZ, cW,
                            sinf(hp), 0.0f, 0.0f, cosf(hp),
                            tX, tY, tZ, tW);
                    cX = tX; cY = tY; cZ = tZ; cW = tW;
                }
                if (corr.yawEnabled) {
                    const float yaw = (renderEye == 0) ? corr.yawDeltaRad : -corr.yawDeltaRad;
                    const float hy = yaw * 0.5f;
                    float tX, tY, tZ, tW;
                    MulQuat(cX, cY, cZ, cW,
                            0.0f, 0.0f, sinf(hy), cosf(hy),
                            tX, tY, tZ, tW);
                    cX = tX; cY = tY; cZ = tZ; cW = tW;
                }
                outX = cX; outY = cY; outZ = cZ; outW = cW;
            }
        }

        if (false) {
            // Extract total Yaw and Pitch, discarding Roll
            const float fx = 2.0f * (outX * outZ + outY * outW);
            const float fy = 2.0f * (outY * outZ - outX * outW);
            const float fz = 1.0f - 2.0f * (outX * outX + outY * outY);

            float finalPitch = asinf(fz < -1.0f ? -1.0f : (fz > 1.0f ? 1.0f : fz));
            float finalYaw = atan2f(-fx, fy);

            const float fcy = cosf(finalYaw * 0.5f);
            const float fsy = sinf(finalYaw * 0.5f);
            const float fcp = cosf(finalPitch * 0.5f);
            const float fsp = sinf(finalPitch * 0.5f);

            outX = fcy * fsp;
            outY = fsy * fsp;
            outZ = fsy * fcp;
            outW = fcy * fcp;
        }

        NormalizeQuat(outX, outY, outZ, outW);
        qx = outX;
        qy = outY;
        qz = outZ;
        qw = outW;
        // Skip the HMD orientation write on the shot frame (or always, mode 1) so the game's
        // native aim/snap drives the camera -> the bullet follows the controller/stick aim.
        if (!skipHmdOrientation) {
            quat[0] = qx;
            quat[1] = qy;
            quat[2] = qz;
            quat[3] = qw;
        }
    }

    // In a menu, also skip the HMD POSITION injection (not just orientation): the
    // map/menu must be a flat, static 2D panel. Moving the camera position with the
    // head shifts the rendered map background while its pins are projected for a
    // fixed position -> pins drift off the map.
    if (hasXR && !menuOpen) {
        const bool allowGameCameraTranslation = g_liveControls.xr3DofMovement == 0;
        const float posScale = allowGameCameraTranslation ? 1.0f * GetWorldScale() : 0.0f;

        // Map OpenXR local position into game-local camera space first:
        // XR: X=right, Y=up, -Z=forward
        // Game local here: X=right, Y=forward, Z=up
        // The BAKED camera->head offset (auto-measured by the plugin, persisted) shifts the view
        // back onto the avatar's head; the Head sliders are added ON TOP (they stay 0 after baking).
        float camBake[3] = { 0.0f, 0.0f, 0.0f };
        if (allowGameCameraTranslation) OpenXRManager::Get().GetCameraOffset(camBake);
        const float localRight = xrPose.posX * posScale +
            (allowGameCameraTranslation ? (g_liveControls.xrHeadOffsetX + camBake[0]) : 0.0f);
        const float localForward = -xrPose.posZ * posScale +
            (allowGameCameraTranslation ? (g_liveControls.xrHeadOffsetY + camBake[1]) : 0.0f);
        const float localUp = xrPose.posY * posScale +
            (allowGameCameraTranslation ? (g_liveControls.xrHeadOffsetZ + camBake[2]) : 0.0f);

        // Use a perfectly level heading matrix for translation to avoid sliding into the floor if pitched
        const float flatYaw = atan2f(-bodyGameForwardX, bodyGameForwardY);
        const float flatCy = cosf(flatYaw);
        const float flatSy = sinf(flatYaw);

        const float flatRightX = flatCy;
        const float flatRightY = flatSy;
        const float flatRightZ = 0.0f;

        const float flatForwardX = -flatSy;
        const float flatForwardY = flatCy;
        const float flatForwardZ = 0.0f;

        const float flatUpX = 0.0f;
        const float flatUpY = 0.0f;
        const float flatUpZ = 1.0f;

        const float worldDeltaX = flatRightX * localRight + flatForwardX * localForward + flatUpX * localUp;
        const float worldDeltaY = flatRightY * localRight + flatForwardY * localForward + flatUpY * localUp;
        const float worldDeltaZ = flatRightZ * localRight + flatForwardZ * localForward + flatUpZ * localUp;

        posFP[0] += static_cast<int32_t>(worldDeltaX * 65536.0f);
        posFP[1] += static_cast<int32_t>(worldDeltaY * 65536.0f);
        posFP[2] += static_cast<int32_t>(worldDeltaZ * 65536.0f);

        if (g_verboseLog && (g_locateCameraHits % 600) == 1) {
            Log("LocateCamera translation: allow=%d posScale=%.4f local=(%.4f, %.4f, %.4f) skippedHead=(%.4f, %.4f, %.4f)\n",
                allowGameCameraTranslation ? 1 : 0,
                posScale,
                localRight,
                localForward,
                localUp,
                g_liveControls.xrHeadOffsetX,
                g_liveControls.xrHeadOffsetY,
                g_liveControls.xrHeadOffsetZ);
        }
    }

    // Per-eye stereo separation for AER uses the runtime's
    // ACTUAL per-eye eye-pose translations, not a synthetic +/-halfIpd scalar.
    // We therefore prefer the current runtime eye-center offset from
    // OpenXRManager (eye pose minus center-eye), scaled by WorldScale/IPDScale/
    // StereoScale, then rotate that full local offset into world using the
    // located camera basis. This preserves asymmetric runtime frusta / tiny
    // non-X offsets with the runtime's own IPD. Fallback to the
    // old right*halfIpd path only if the runtime eye offsets are unavailable.
    // NOTE: IPD shift is applied REGARDLESS of menuOpen. The shared[63] collision
    // (delta-quaternion float bits) makes menuOpen accidentally true on most frames,
    // which previously prevented eye alternation entirely (game rendered only one
    // eye). Applying IPD always ensures AER eye alternation works even while the
    // collision exists. The actual menu/map path (shared[70]) suspends resolution
    // overrides separately and does not need IPD suppression here.
    // LATE IPD SHIFT: compute the per-eye stereo offset here but DO NOT move the
    // located camera. posFP feeds the engine's IK/physics/gameplay head; shifting
    // it ±halfIPD every frame is what makes VRIK thrash. We store the (eye-signed)
    // shift and let OnFinalCameraCallback add it to the FINAL render camera only,
    // post-IK, just before projection. Render output is
    // unchanged (the final camera ends up at the same place); only the IK/physics
    // head now stays at the stable center.
    int32_t ipdShiftFP[3] = {0, 0, 0};
    if (hasXR && OpenXRManager::Get().IsAERSubmitEnabled()) {
        const int renderEye = OpenXRManager::Get().GetCurrentRenderEyeIndex();
        XrVector3f eyeLocalXR{};
        bool usedRuntimeEyeOffset = false;
        if (OpenXRManager::Get().GetCurrentEyeCenterOffset(renderEye, &eyeLocalXR)) {
            float ipdScale = g_liveControls.xrIpdScale;
            if (!(ipdScale > 0.0f)) ipdScale = 1.0f;
            float stereoScale = g_liveControls.xrStereoScale;
            if (!(stereoScale > 0.0f)) stereoScale = 1.0f;
            const float eyeScale = GetWorldScale() * ipdScale * stereoScale;
            const float localRight = eyeLocalXR.x * eyeScale;
            const float localForward = -eyeLocalXR.z * eyeScale;
            const float localUp = eyeLocalXR.y * eyeScale;
            float basis[12] = {};
            BuildGameViewRowsFromQuaternion(quat, basis);
            const float worldDeltaX = basis[0] * localRight + basis[8] * localForward + basis[4] * localUp;
            const float worldDeltaY = basis[1] * localRight + basis[9] * localForward + basis[5] * localUp;
            const float worldDeltaZ = basis[2] * localRight + basis[10] * localForward + basis[6] * localUp;
            ipdShiftFP[0] = static_cast<int32_t>(worldDeltaX * 65536.0f);
            ipdShiftFP[1] = static_cast<int32_t>(worldDeltaY * 65536.0f);
            ipdShiftFP[2] = static_cast<int32_t>(worldDeltaZ * 65536.0f);
            usedRuntimeEyeOffset = true;
            if (g_verboseLog && (g_locateCameraHits % 600) == 1) {
                Log("LocateCamera IPD (runtime-eye-offset): eye=%d localXR=(%.4f, %.4f, %.4f) scale=%.4f world=(%.4f, %.4f, %.4f)\n",
                    renderEye,
                    eyeLocalXR.x, eyeLocalXR.y, eyeLocalXR.z,
                    eyeScale,
                    worldDeltaX, worldDeltaY, worldDeltaZ);
            }
        }
        if (!usedRuntimeEyeOffset) {
            float right[3] = {};
            ComputeRightVectorFromQuaternion(quat, right);
            if (IsPlausibleUnitVector3(right)) {
                const float halfIpd = GetDesiredHalfIpd();
                const float eyeSign = (renderEye == 0) ? -1.0f : 1.0f;
                const float ipdShift = halfIpd * eyeSign;
                ipdShiftFP[0] = static_cast<int32_t>(right[0] * ipdShift * 65536.0f);
                ipdShiftFP[1] = static_cast<int32_t>(right[1] * ipdShift * 65536.0f);
                ipdShiftFP[2] = static_cast<int32_t>(right[2] * ipdShift * 65536.0f);
                if (g_verboseLog && (g_locateCameraHits % 600) == 1) {
                    Log("LocateCamera IPD (fallback right*halfIpd): eye=%d halfIpd=%.4f right=(%.3f, %.3f, %.3f) shift=%.4f\n",
                        renderEye,
                        halfIpd, right[0], right[1], right[2], ipdShift);
                }
            }
        }
    }

    // Located camera = head CENTER (no IPD). IK/physics/VRIK read this.
    g_lastLocatePosFP[0] = posFP[0];
    g_lastLocatePosFP[1] = posFP[1];
    g_lastLocatePosFP[2] = posFP[2];
    // Per-eye shift carried to OnFinalCameraCallback for late application.
    g_lastIpdShiftFP[0] = ipdShiftFP[0];
    g_lastIpdShiftFP[1] = ipdShiftFP[1];
    g_lastIpdShiftFP[2] = ipdShiftFP[2];
    g_lastLocateQuat[0] = quat[0];
    g_lastLocateQuat[1] = quat[1];
    g_lastLocateQuat[2] = quat[2];
    g_lastLocateQuat[3] = quat[3];
    ++g_lastLocateSeq;

    // Publish the located camera + a controller-aim quaternion for the plugin's ShotSnap.
    // controllerAim = bodyYaw (X) controllerGame, built EXACTLY like the camera quat above
    // (same x,-z,y axis map + the same gameYaw), so the bullet, when this quat is bracketed
    // into the located camera during a shot, flies down the controller while the view (which
    // reads the HMD quat we just wrote) stays on the head.
    if (hasXR) {
        if (float* sh = GetShotShared()) {
            OpenXRHeadPose handPose{};
            const bool hasHand = OpenXRManager::Get().GetHandPose(1, &handPose) && handPose.valid;
            if (hasHand) {
                const float gameYaw2 = atan2f(-bodyGameForwardX, bodyGameForwardY);
                const float cy2 = cosf(gameYaw2 * 0.5f);
                const float sy2 = sinf(gameYaw2 * 0.5f);
                const float cgX = handPose.oriX;
                const float cgY = -handPose.oriZ;
                const float cgZ = handPose.oriY;
                const float cgW = handPose.oriW;
                float aX, aY, aZ, aW;
                MulQuat(0.0f, 0.0f, sy2, cy2, cgX, cgY, cgZ, cgW, aX, aY, aZ, aW);
                NormalizeQuat(aX, aY, aZ, aW);
                const uintptr_t camAddr = reinterpret_cast<uintptr_t>(rbxPtr);
                uint32_t lo = static_cast<uint32_t>(camAddr & 0xFFFFFFFFu);
                uint32_t hi = static_cast<uint32_t>(camAddr >> 32);
                memcpy(&sh[51], &lo, 4);
                memcpy(&sh[52], &hi, 4);
                sh[53] = aX; sh[54] = aY; sh[55] = aZ; sh[56] = aW;
                // Controller FORWARD as a WORLD direction vector for the fire-shot hook:
                // rotate game-forward (0,1,0) by the aim quat.
                // v = q * (0,1,0) * q^-1, expanded:
                const float fwX = 2.0f * (aX * aY - aZ * aW);
                const float fwY = 1.0f - 2.0f * (aX * aX + aZ * aZ);
                const float fwZ = 2.0f * (aY * aZ + aX * aW);
                sh[60] = fwX; sh[61] = fwY; sh[62] = fwZ;
                // DELTA quat = inv(hmd_game) * controller_game  (both remapped x,-z,y,w to game axes).
                // The plugin multiplies the provider's ORIGINAL camera quat by this:
                //   qNew = camera * delta = (bodyYaw*hmd) * (inv(hmd)*controller) = bodyYaw*controller
                // -> bullet flies down the controller, in correct game-world space (pivots off the
                //    known-correct camera orientation instead of rebuilding world from scratch).
                {
                    // PROPER OpenXR->game for a RELATIVE rotation. The component swap (x,-z,y,w) used
                    // elsewhere is only valid for absolute look quats, NOT for a rotation delta (that
                    // needs a similarity transform P*q*P^-1). So: compute the delta in RAW XR space,
                    // then conjugate it into game space by P = rotX(+90deg) (xr->game axis map).
                    // delta_xr = inv(head_xr) * hand_xr   (controller relative to head, headset space)
                    float dxrX, dxrY, dxrZ, dxrW;
                    MulQuat(-xrPose.oriX, -xrPose.oriY, -xrPose.oriZ, xrPose.oriW,   // inv(head_xr)
                            handPose.oriX, handPose.oriY, handPose.oriZ, handPose.oriW,
                            dxrX, dxrY, dxrZ, dxrW);
                    // delta_game = P * delta_xr * P^-1 ; P=(0.70710678,0,0,0.70710678)
                    const float pX = 0.70710678f, pW = 0.70710678f;
                    float t1X, t1Y, t1Z, t1W;
                    MulQuat(pX, 0.0f, 0.0f, pW, dxrX, dxrY, dxrZ, dxrW, t1X, t1Y, t1Z, t1W);   // P * delta
                    float dX, dY, dZ, dW;
                    MulQuat(t1X, t1Y, t1Z, t1W, -pX, 0.0f, 0.0f, pW, dX, dY, dZ, dW);            // * P^-1
                    NormalizeQuat(dX, dY, dZ, dW);
                    sh[63] = dX; sh[64] = dY; sh[65] = dZ; sh[66] = dW;
                }
                sh[50] = static_cast<float>(g_lastLocateSeq & 0xFFFFFF); // valid/heartbeat
            }
        }
    }
}

bool InstallLocateCameraHook() {
    const char* pattern = "\xF3\x0F\x11\x43\x20\x48\x8D\x54\x24\x20\x48\x8B\x06";
    const char* mask = "xxxxxxxxxxxxx";
    uint8_t* found = static_cast<uint8_t*>(FindPattern("Cyberpunk2077.exe", pattern, mask));
    if (!found) return false;

    constexpr int replaceLen = 10; 
    void* tramp = AllocateTrampoline(found, 512);
    if (!tramp) return false;

    uint8_t* code = static_cast<uint8_t*>(tramp);
    int pos = 0;

    // --- CALL C++ CALLBACK ---
    code[pos++] = 0x9C; // pushfq
    code[pos++] = 0x50; // push rax
    code[pos++] = 0x51; // push rcx
    code[pos++] = 0x52; // push rdx
    code[pos++] = 0x41; code[pos++] = 0x50; // push r8
    code[pos++] = 0x41; code[pos++] = 0x51; // push r9
    code[pos++] = 0x41; code[pos++] = 0x52; // push r10
    code[pos++] = 0x41; code[pos++] = 0x53; // push r11
    code[pos++] = 0x55; // push rbp

    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x40; // sub rsp, 40h
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x04; code[pos++] = 0x24; // movups [rsp+00h], xmm0
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10; // movups [rsp+10h], xmm1
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20; // movups [rsp+20h], xmm2
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30; // movups [rsp+30h], xmm3

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xE5; // mov rbp, rsp
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xE4; code[pos++] = 0xF0; // and rsp, -16
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x20; // sub rsp, 20h

    // Set arg1 (rcx) = rbx
    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xD9; // mov rcx, rbx
    // Set arg2 (xmm1) = xmm0 (since float args go in xmm registers, xmm1 is 2nd arg)
    code[pos++] = 0x0F; code[pos++] = 0x28; code[pos++] = 0xC8; // movaps xmm1, xmm0

    WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(OnLocateCameraCallback));
    code[pos++] = 0xFF; code[pos++] = 0xD0; // call rax

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xEC; // mov rsp, rbp

    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x04; code[pos++] = 0x24; // movups xmm0, [rsp+00h]
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10; // movups xmm1, [rsp+10h]
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20; // movups xmm2, [rsp+20h]
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30; // movups xmm3, [rsp+30h]
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xC4; code[pos++] = 0x40; // add rsp, 40h

    code[pos++] = 0x5D; // pop rbp
    code[pos++] = 0x41; code[pos++] = 0x5B; // pop r11
    code[pos++] = 0x41; code[pos++] = 0x5A; // pop r10
    code[pos++] = 0x41; code[pos++] = 0x59; // pop r9
    code[pos++] = 0x41; code[pos++] = 0x58; // pop r8
    code[pos++] = 0x5A; // pop rdx
    code[pos++] = 0x59; // pop rcx
    code[pos++] = 0x58; // pop rax
    code[pos++] = 0x9D; // popfq

    // Original instructions:
    // movss [rbx+20h], xmm0
    code[pos++] = 0xF3; code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x43; code[pos++] = 0x20;
    // lea rdx, [rsp+20h]
    code[pos++] = 0x48; code[pos++] = 0x8D; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20;

    // jmp back
    code[pos++] = 0xE9;
    *reinterpret_cast<int32_t*>(code + pos) = static_cast<int32_t>((found + replaceLen) - (code + pos + 4));
    pos += 4;

    DWORD oldProtect;
    VirtualProtect(found, replaceLen, PAGE_EXECUTE_READWRITE, &oldProtect);
    found[0] = 0xE9;
    *reinterpret_cast<int32_t*>(found + 1) = static_cast<int32_t>(code - (found + 5));
    for (int i = 5; i < replaceLen; ++i) found[i] = 0x90;
    VirtualProtect(found, replaceLen, oldProtect, &oldProtect);
    FlushInstructionCache(GetCurrentProcess(), found, replaceLen);
    return true;
}

static uint64_t g_patchCameraHits = 0;

extern "C" void __fastcall OnPatchCameraCallback(float* cameraState, void* ownerState) {
    g_patchCameraHits++;

    if (!cameraState || reinterpret_cast<uintptr_t>(cameraState) < 0x10000) return;

    float quat[4] = {};
    float posA[4] = {};
    float posB[4] = {};
    if (!ReadFloatArraySafe(cameraState + 0, quat, 4) ||
        !ReadFloatArraySafe(cameraState + 4, posA, 4) ||
        !ReadFloatArraySafe(cameraState + 8, posB, 4)) {
        return;
    }

    float right[3] = {};
    float shift = GetDesiredHalfIpd();
    bool shifted = false;

    if (shift != 0.0f &&
        IsPlausibleUnitQuaternion(quat) &&
        IsPlausibleCameraSpan(posA, posB)) {
        ComputeRightVectorFromQuaternion(quat, right);
        if (IsPlausibleUnitVector3(right)) {
            if (OpenXRManager::Get().IsAERSubmitEnabled()) {
                shift *= (OpenXRManager::Get().GetCurrentRenderEyeIndex() == 0) ? -1.0f : 1.0f;
            } else if ((g_patchCameraHits % 2) == 0) {
                shift = -shift;
            }

            const uintptr_t stateAddr = reinterpret_cast<uintptr_t>(cameraState);
            const float dx = right[0] * shift;
            const float dy = right[1] * shift;
            const float dz = right[2] * shift;

            WriteFloatSafe(stateAddr + 0x10, posA[0] + dx);
            WriteFloatSafe(stateAddr + 0x14, posA[1] + dy);
            WriteFloatSafe(stateAddr + 0x18, posA[2] + dz);
            WriteFloatSafe(stateAddr + 0x20, posB[0] + dx);
            WriteFloatSafe(stateAddr + 0x24, posB[1] + dy);
            WriteFloatSafe(stateAddr + 0x28, posB[2] + dz);
            shifted = true;
        }
    }

    if ((g_patchCameraHits % 600) == 0) {
        uint8_t ownerFlag = 0xFF;
        if (ownerState && reinterpret_cast<uintptr_t>(ownerState) >= 0x10000) {
            ReadU8Safe(reinterpret_cast<uintptr_t>(ownerState) + 0xB1, &ownerFlag);
        }

        if (g_verboseLog) Log("PatchCamera state @%p owner=%p flagB1=%u Q=(%.3f, %.3f, %.3f, %.3f) P0=(%.3f, %.3f, %.3f) P1=(%.3f, %.3f, %.3f) R=(%.3f, %.3f, %.3f) shift=%.4f applied=%d\n",
            cameraState,
            ownerState,
            static_cast<unsigned>(ownerFlag),
            quat[0], quat[1], quat[2], quat[3],
            posA[0], posA[1], posA[2],
            posB[0], posB[1], posB[2],
            right[0], right[1], right[2],
            shift,
            shifted ? 1 : 0);
    }
}

bool InstallPatchCameraHook() {
    const char* pattern = "\x0F\x11\x02\x80\xBE\xB1\x00\x00\x00\x00\x89\x45\x88";
    const char* mask = "xxxxxxxxxxxxx";
    uint8_t* found = static_cast<uint8_t*>(FindPattern("Cyberpunk2077.exe", pattern, mask));
    if (!found) return false;

    constexpr int replaceLen = 10; // 0F 11 02 80 BE B1 00 00 00 00
    void* tramp = AllocateTrampoline(found, 512);
    if (!tramp) return false;

    uint8_t* code = static_cast<uint8_t*>(tramp);
    int pos = 0;

    // push rax
    code[pos++] = 0x50;
    WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(g_telemetry) + kPatchTelemetryOffset);
    code[pos++] = 0xFF; code[pos++] = 0x00; // inc dword ptr [rax+0]
    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0x50; code[pos++] = 0x08; // mov [rax+8], rdx
    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0x70; code[pos++] = 0x10; // mov [rax+10h], rsi
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x40; code[pos++] = 0x18; // movups [rax+18h], xmm0
    code[pos++] = 0x58; // pop rax

    // Original instructions:
    // movups [rdx], xmm0
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x02;

    // --- CALL C++ CALLBACK ---
    // Save volatile registers
    code[pos++] = 0x9C; // pushfq
    code[pos++] = 0x50; // push rax
    code[pos++] = 0x51; // push rcx
    code[pos++] = 0x52; // push rdx
    code[pos++] = 0x41; code[pos++] = 0x50; // push r8
    code[pos++] = 0x41; code[pos++] = 0x51; // push r9
    code[pos++] = 0x41; code[pos++] = 0x52; // push r10
    code[pos++] = 0x41; code[pos++] = 0x53; // push r11
    code[pos++] = 0x55; // push rbp

    // Save xmm0-xmm3
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x40; // sub rsp, 40h
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x04; code[pos++] = 0x24; // movups [rsp+00h], xmm0
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10; // movups [rsp+10h], xmm1
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20; // movups [rsp+20h], xmm2
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30; // movups [rsp+30h], xmm3

    // Align stack and allocate shadow space
    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xE5; // mov rbp, rsp
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xE4; code[pos++] = 0xF0; // and rsp, -16 (0xFFFFFFFFFFFFFFF0)
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x20; // sub rsp, 20h

    // Set arg1 (rcx) = rdx (camera state), arg2 (rdx) = rsi (owner state)
    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xD1; // mov rcx, rdx
    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xF2; // mov rdx, rsi

    // Call OnPatchCameraCallback
    WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(OnPatchCameraCallback));
    code[pos++] = 0xFF; code[pos++] = 0xD0; // call rax

    // Restore unaligned stack pointer
    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xEC; // mov rsp, rbp

    // Restore xmm0-xmm3
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x04; code[pos++] = 0x24; // movups xmm0, [rsp+00h]
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10; // movups xmm1, [rsp+10h]
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20; // movups xmm2, [rsp+20h]
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30; // movups xmm3, [rsp+30h]
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xC4; code[pos++] = 0x40; // add rsp, 40h

    // Restore volatile registers
    code[pos++] = 0x5D; // pop rbp
    code[pos++] = 0x41; code[pos++] = 0x5B; // pop r11
    code[pos++] = 0x41; code[pos++] = 0x5A; // pop r10
    code[pos++] = 0x41; code[pos++] = 0x59; // pop r9
    code[pos++] = 0x41; code[pos++] = 0x58; // pop r8
    code[pos++] = 0x5A; // pop rdx
    code[pos++] = 0x59; // pop rcx
    code[pos++] = 0x58; // pop rax
    code[pos++] = 0x9D; // popfq

    // End original instruction block
    code[pos++] = 0x80; code[pos++] = 0xBE; code[pos++] = 0xB1; code[pos++] = 0x00;
    code[pos++] = 0x00; code[pos++] = 0x00; code[pos++] = 0x00; // cmp byte ptr [rsi+0B1h], 0

    // jmp back
    code[pos++] = 0xE9;
    *reinterpret_cast<int32_t*>(code + pos) = static_cast<int32_t>((found + replaceLen) - (code + pos + 4));
    pos += 4;

    DWORD oldProtect;
    VirtualProtect(found, replaceLen, PAGE_EXECUTE_READWRITE, &oldProtect);
    found[0] = 0xE9;
    *reinterpret_cast<int32_t*>(found + 1) = static_cast<int32_t>(code - (found + 5));
    for (int i = 5; i < replaceLen; ++i) found[i] = 0x90;
    VirtualProtect(found, replaceLen, oldProtect, &oldProtect);
    FlushInstructionCache(GetCurrentProcess(), found, replaceLen);
    return true;
}

static uint64_t g_finalCameraHits = 0;
extern "C" void __fastcall OnFinalCameraCallback(float* rsiPtr) {
    g_finalCameraHits++;
    if (g_telemetry) {
        g_telemetry->finalHits = static_cast<uint32_t>(g_finalCameraHits);
        g_telemetry->finalRsi = reinterpret_cast<uintptr_t>(rsiPtr);
    }

    const uint32_t locateSeq = g_lastLocateSeq;
    if (locateSeq != 0) {
        g_renderedSeq = locateSeq;
        g_renderedEye = static_cast<int>(g_locateEyeBySeq[locateSeq % 256] & 1);
    }

    if (!OpenXRManager::Get().IsAERSubmitEnabled()) return;
    if (!rsiPtr || reinterpret_cast<uintptr_t>(rsiPtr) < 0x10000) return;

    float locateQuat[4] = {
        g_lastLocateQuat[0],
        g_lastLocateQuat[1],
        g_lastLocateQuat[2],
        g_lastLocateQuat[3]
    };
    if (locateSeq != 0) {
        // LATE IPD SHIFT: the located camera is the head CENTER; add the per-eye
        // stereo offset HERE, on the final render camera only (post-IK, pre-
        // projection). VRIK/physics
        // never saw the shifted head, yet stereo is fully preserved in the render.
        int32_t* finalPosFP = reinterpret_cast<int32_t*>(rsiPtr);
        finalPosFP[0] = g_lastLocatePosFP[0] + g_lastIpdShiftFP[0];
        finalPosFP[1] = g_lastLocatePosFP[1] + g_lastIpdShiftFP[1];
        finalPosFP[2] = g_lastLocatePosFP[2] + g_lastIpdShiftFP[2];

        if (IsPlausibleUnitQuaternion(locateQuat)) {
            ApplyFinalCameraOrientationFromQuat(rsiPtr, locateQuat);
        }
    }

    if (!g_verboseLog || (g_finalCameraHits % 600) != 1) return;

    float values[24] = {};
    float cameraMtx[16] = {};
    float cameraViewA[12] = {};
    float cameraViewB[16] = {};
    if (!ReadFloatArraySafe(rsiPtr, values, 24)) return;
    ReadFloatArraySafe(rsiPtr + 20, cameraMtx, 16);   // +0x50
    ReadFloatArraySafe(rsiPtr + 68, cameraViewA, 12); // +0x110
    ReadFloatArraySafe(rsiPtr + 204, cameraViewB, 16); // +0x330

    const int32_t* finalPosFP = reinterpret_cast<const int32_t*>(rsiPtr);
    const float posScale = 1.0f / 65536.0f;

    Log("FinalCamera probe: hit=%llu rsi=%p eye=%d f40=%.6f f44=%.6f pos=(%.3f, %.3f, %.3f) locateSeq=%u locQ=(%.3f, %.3f, %.3f, %.3f)\n",
        static_cast<unsigned long long>(g_finalCameraHits),
        rsiPtr,
        OpenXRManager::Get().GetCurrentRenderEyeIndex(),
        values[16],
        values[17],
        static_cast<float>(finalPosFP[0]) * posScale,
        static_cast<float>(finalPosFP[1]) * posScale,
        static_cast<float>(finalPosFP[2]) * posScale,
        locateSeq,
        locateQuat[0], locateQuat[1], locateQuat[2], locateQuat[3]);

    if (LooksProjectionLike(values, 16)) {
        LogMatrix4x4("FinalCamera matrix candidate:", values);
    } else {
        Log("FinalCamera raw[0..15]: %.6f %.6f %.6f %.6f  %.6f %.6f %.6f %.6f  %.6f %.6f %.6f %.6f  %.6f %.6f %.6f %.6f\n",
            values[0], values[1], values[2], values[3],
            values[4], values[5], values[6], values[7],
            values[8], values[9], values[10], values[11],
            values[12], values[13], values[14], values[15]);
    }

    Log("FinalCamera mtx+0x50: %.6f %.6f %.6f %.6f  %.6f %.6f %.6f %.6f  %.6f %.6f %.6f %.6f  %.6f %.6f %.6f %.6f\n",
        cameraMtx[0], cameraMtx[1], cameraMtx[2], cameraMtx[3],
        cameraMtx[4], cameraMtx[5], cameraMtx[6], cameraMtx[7],
        cameraMtx[8], cameraMtx[9], cameraMtx[10], cameraMtx[11],
        cameraMtx[12], cameraMtx[13], cameraMtx[14], cameraMtx[15]);
    Log("FinalCamera view+0x110: %.6f %.6f %.6f %.6f  %.6f %.6f %.6f %.6f  %.6f %.6f %.6f %.6f\n",
        cameraViewA[0], cameraViewA[1], cameraViewA[2], cameraViewA[3],
        cameraViewA[4], cameraViewA[5], cameraViewA[6], cameraViewA[7],
        cameraViewA[8], cameraViewA[9], cameraViewA[10], cameraViewA[11]);
    Log("FinalCamera view+0x330: %.6f %.6f %.6f %.6f  %.6f %.6f %.6f %.6f  %.6f %.6f %.6f %.6f  %.6f %.6f %.6f %.6f\n",
        cameraViewB[0], cameraViewB[1], cameraViewB[2], cameraViewB[3],
        cameraViewB[4], cameraViewB[5], cameraViewB[6], cameraViewB[7],
        cameraViewB[8], cameraViewB[9], cameraViewB[10], cameraViewB[11],
        cameraViewB[12], cameraViewB[13], cameraViewB[14], cameraViewB[15]);
}

bool InstallFinalCameraHook() {
    const char* pattern = "\xF3\x44\x0F\x5E\x4E\x40\x49\x81\xC0\x20\x0F\x00\x00";
    const char* mask = "xxxxxxxxxxxxx";
    uint8_t* found = static_cast<uint8_t*>(FindPattern("Cyberpunk2077.exe", pattern, mask));
    if (!found) return false;

    constexpr int replaceLen = 13; 
    void* tramp = AllocateTrampoline(found, 512);
    if (!tramp) return false;

    uint8_t* code = static_cast<uint8_t*>(tramp);
    int pos = 0;

    // --- CALL C++ CALLBACK ---
    code[pos++] = 0x9C; // pushfq
    code[pos++] = 0x50; // push rax
    code[pos++] = 0x51; // push rcx
    code[pos++] = 0x52; // push rdx
    code[pos++] = 0x41; code[pos++] = 0x50; // push r8
    code[pos++] = 0x41; code[pos++] = 0x51; // push r9
    code[pos++] = 0x41; code[pos++] = 0x52; // push r10
    code[pos++] = 0x41; code[pos++] = 0x53; // push r11
    code[pos++] = 0x55; // push rbp

    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x40; // sub rsp, 40h
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x04; code[pos++] = 0x24; // movups [rsp+00h], xmm0
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10; // movups [rsp+10h], xmm1
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20; // movups [rsp+20h], xmm2
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30; // movups [rsp+30h], xmm3

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xE5; // mov rbp, rsp
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xE4; code[pos++] = 0xF0; // and rsp, -16
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x20; // sub rsp, 20h

    // Set arg1 (rcx) = rsi
    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xF1; // mov rcx, rsi

    WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(OnFinalCameraCallback));
    code[pos++] = 0xFF; code[pos++] = 0xD0; // call rax

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xEC; // mov rsp, rbp

    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x04; code[pos++] = 0x24; // movups xmm0, [rsp+00h]
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10; // movups xmm1, [rsp+10h]
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20; // movups xmm2, [rsp+20h]
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30; // movups xmm3, [rsp+30h]
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xC4; code[pos++] = 0x40; // add rsp, 40h

    code[pos++] = 0x5D; // pop rbp
    code[pos++] = 0x41; code[pos++] = 0x5B; // pop r11
    code[pos++] = 0x41; code[pos++] = 0x5A; // pop r10
    code[pos++] = 0x41; code[pos++] = 0x59; // pop r9
    code[pos++] = 0x41; code[pos++] = 0x58; // pop r8
    code[pos++] = 0x5A; // pop rdx
    code[pos++] = 0x59; // pop rcx
    code[pos++] = 0x58; // pop rax
    code[pos++] = 0x9D; // popfq

    // Original instructions:
    // divss xmm9, [rsi+40h]
    code[pos++] = 0xF3; code[pos++] = 0x44; code[pos++] = 0x0F; code[pos++] = 0x5E; code[pos++] = 0x4E; code[pos++] = 0x40;
    // add r8, 0F20h
    code[pos++] = 0x49; code[pos++] = 0x81; code[pos++] = 0xC0; code[pos++] = 0x20; code[pos++] = 0x0F; code[pos++] = 0x00; code[pos++] = 0x00;

    // jmp back
    code[pos++] = 0xE9;
    *reinterpret_cast<int32_t*>(code + pos) = static_cast<int32_t>((found + replaceLen) - (code + pos + 4));
    pos += 4;

    DWORD oldProtect;
    VirtualProtect(found, replaceLen, PAGE_EXECUTE_READWRITE, &oldProtect);
    found[0] = 0xE9;
    *reinterpret_cast<int32_t*>(found + 1) = static_cast<int32_t>(code - (found + 5));
    for (int i = 5; i < replaceLen; ++i) found[i] = 0x90;
    VirtualProtect(found, replaceLen, oldProtect, &oldProtect);
    FlushInstructionCache(GetCurrentProcess(), found, replaceLen);
    return true;
}

static uint64_t g_pitchHookHits = 0;

extern "C" void __fastcall OnPitchHookCallback(void* pitchState, float originalPitch) {
    g_pitchHookHits++;

    // Mouse-Y pitch in CP2077 also drives a constrained camera pivot offset, which
    // moves the head toward/away from the body in VR. Keep the game pitch neutral;
    // HMD pitch is applied visually in LocateCamera instead.
    const float desiredPitch = 0.0f;

    g_pitchOverrideValue = desiredPitch;

    if (g_verboseLog && (g_pitchHookHits % 600) == 1) {
        float minPitch = 0.0f;
        float maxPitch = 0.0f;
        ReadFloatSafe(reinterpret_cast<uintptr_t>(pitchState) + 0x14, &minPitch);
        ReadFloatSafe(reinterpret_cast<uintptr_t>(pitchState) + 0x18, &maxPitch);
        Log("Pitch hook: state=%p original=%.6f neutral=%.6f clamp=[%.6f, %.6f]\n",
            pitchState,
            originalPitch,
            desiredPitch,
            minPitch,
            maxPitch);
    }
}

bool InstallPitchHook() {
    const char* pattern = "\xF3\x0F\x10\x4F\x14\xF3\x0F\x5F\xC8\xF3\x0F\x5D\x4F\x18";
    const char* mask = "xxxxxxxxxxxxxx";
    uint8_t* found = static_cast<uint8_t*>(FindPattern("Cyberpunk2077.exe", pattern, mask));
    if (!found) return false;

    constexpr int replaceLen = 14;
    void* tramp = AllocateTrampoline(found, 512);
    if (!tramp) return false;

    uint8_t* code = static_cast<uint8_t*>(tramp);
    int pos = 0;

    code[pos++] = 0x9C;
    code[pos++] = 0x50;
    code[pos++] = 0x51;
    code[pos++] = 0x52;
    code[pos++] = 0x41; code[pos++] = 0x50;
    code[pos++] = 0x41; code[pos++] = 0x51;
    code[pos++] = 0x41; code[pos++] = 0x52;
    code[pos++] = 0x41; code[pos++] = 0x53;
    code[pos++] = 0x55;

    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x40;
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x04; code[pos++] = 0x24;
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10;
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20;
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30;

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xE5;
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xE4; code[pos++] = 0xF0;
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x20;

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xF9; // mov rcx, rdi
    code[pos++] = 0x0F; code[pos++] = 0x28; code[pos++] = 0xC8; // movaps xmm1, xmm0
    WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(OnPitchHookCallback));
    code[pos++] = 0xFF; code[pos++] = 0xD0;

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xEC;

    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x04; code[pos++] = 0x24;
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10;
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20;
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30;
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xC4; code[pos++] = 0x40;

    code[pos++] = 0x5D;
    code[pos++] = 0x41; code[pos++] = 0x5B;
    code[pos++] = 0x41; code[pos++] = 0x5A;
    code[pos++] = 0x41; code[pos++] = 0x59;
    code[pos++] = 0x41; code[pos++] = 0x58;
    code[pos++] = 0x5A;
    code[pos++] = 0x59;
    code[pos++] = 0x58;
    code[pos++] = 0x9D;

    WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(&g_pitchOverrideValue));
    code[pos++] = 0xF3; code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x00; // movss xmm0,[rax]

    code[pos++] = 0xF3; code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x4F; code[pos++] = 0x14;
    code[pos++] = 0xF3; code[pos++] = 0x0F; code[pos++] = 0x5F; code[pos++] = 0xC8;
    code[pos++] = 0xF3; code[pos++] = 0x0F; code[pos++] = 0x5D; code[pos++] = 0x4F; code[pos++] = 0x18;

    code[pos++] = 0xE9;
    *reinterpret_cast<int32_t*>(code + pos) = static_cast<int32_t>((found + replaceLen) - (code + pos + 4));
    pos += 4;

    DWORD oldProtect;
    VirtualProtect(found, replaceLen, PAGE_EXECUTE_READWRITE, &oldProtect);
    found[0] = 0xE9;
    *reinterpret_cast<int32_t*>(found + 1) = static_cast<int32_t>(code - (found + 5));
    for (int i = 5; i < replaceLen; ++i) found[i] = 0x90;
    VirtualProtect(found, replaceLen, oldProtect, &oldProtect);
    FlushInstructionCache(GetCurrentProcess(), found, replaceLen);
    return true;
}

static uint64_t g_normalFovHookHits = 0;

extern "C" void __fastcall OnNormalFovHookCallback(void* cameraState, float originalFov) {
    g_normalFovHookHits++;

    const float desiredFov = GetDesiredGameHorizontalFov();
    g_normalFovOverrideValue = desiredFov > 1.0f ? desiredFov : originalFov;

    // Keep the visible camera FOV aligned with the OpenXR runtime, while giving
    // LOD/streaming a wider cone so edge and floor geometry does not pop out.
    g_lodFovOverride = g_normalFovOverrideValue + 30.0f;

    // The camera FOV hook writes the FOV ONLY to camera
    // +0x410 -- which our trampoline already forces via the patched xmm3 -- and never
    // touches +0x414. We used to also write +0x414; that slot is NOT the horizontal
    // FOV, and overwriting it distorted the projection ("world too big", visible in
    // BOTH Mono and AER => a monocular/FOV artifact, not IPD). Leave
    // +0x414 alone.
    //
    // if (cameraState && desiredFov > 1.0f) {
    //     const uintptr_t stateAddr = reinterpret_cast<uintptr_t>(cameraState);
    //     WriteFloatSafe(stateAddr + 0x414, g_normalFovOverrideValue);
    // }

    if (g_verboseLog && (g_normalFovHookHits % 600) == 1) {
        float currentHFov = 0.0f;
        float currentVFov = 0.0f;
        const uintptr_t stateAddr = reinterpret_cast<uintptr_t>(cameraState);
        ReadFloatSafe(stateAddr + 0x410, &currentHFov);
        ReadFloatSafe(stateAddr + 0x414, &currentVFov);
        Log("NormalFOV hook: state=%p original=%.6f desired=%.6f storedH=%.6f storedV=%.6f lodFov=%.6f runtimeHFov=%.6f runtimeIPD=%.6f\n",
            cameraState,
            originalFov,
            g_normalFovOverrideValue,
            currentHFov,
            currentVFov,
            g_lodFovOverride,
            OpenXRManager::Get().GetRuntimeHorizontalFovDeg(),
            OpenXRManager::Get().GetRuntimeIpd());
    }
}

bool InstallNormalFovHook() {
    const char* pattern = "\xF3\x0F\x11\x99\x10\x04\x00\x00\x48\x8B\x91\x60\x03\x00\x00";
    const char* mask = "xxxxxxxxxxxxxxx";
    uint8_t* found = static_cast<uint8_t*>(FindPattern("Cyberpunk2077.exe", pattern, mask));
    if (!found) return false;

    constexpr int replaceLen = 15;
    void* tramp = AllocateTrampoline(found, 512);
    if (!tramp) return false;

    uint8_t* code = static_cast<uint8_t*>(tramp);
    int pos = 0;

    code[pos++] = 0x9C;
    code[pos++] = 0x50;
    code[pos++] = 0x51;
    code[pos++] = 0x52;
    code[pos++] = 0x41; code[pos++] = 0x50;
    code[pos++] = 0x41; code[pos++] = 0x51;
    code[pos++] = 0x41; code[pos++] = 0x52;
    code[pos++] = 0x41; code[pos++] = 0x53;
    code[pos++] = 0x55;

    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x40;
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x04; code[pos++] = 0x24;
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10;
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20;
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30;

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xE5;
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xE4; code[pos++] = 0xF0;
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x20;

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xC9; // mov rcx, rcx
    code[pos++] = 0x0F; code[pos++] = 0x28; code[pos++] = 0xCB; // movaps xmm1, xmm3
    WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(OnNormalFovHookCallback));
    code[pos++] = 0xFF; code[pos++] = 0xD0;

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xEC;

    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x04; code[pos++] = 0x24;
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10;
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20;
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30;
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xC4; code[pos++] = 0x40;

    code[pos++] = 0x5D;
    code[pos++] = 0x41; code[pos++] = 0x5B;
    code[pos++] = 0x41; code[pos++] = 0x5A;
    code[pos++] = 0x41; code[pos++] = 0x59;
    code[pos++] = 0x41; code[pos++] = 0x58;
    code[pos++] = 0x5A;
    code[pos++] = 0x59;
    code[pos++] = 0x58;
    code[pos++] = 0x9D;

    WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(&g_normalFovOverrideValue));
    code[pos++] = 0xF3; code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x18; // movss xmm3,[rax]
    code[pos++] = 0xF3; code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x99; code[pos++] = 0x10; code[pos++] = 0x04; code[pos++] = 0x00; code[pos++] = 0x00;
    code[pos++] = 0x48; code[pos++] = 0x8B; code[pos++] = 0x91; code[pos++] = 0x60; code[pos++] = 0x03; code[pos++] = 0x00; code[pos++] = 0x00;

    code[pos++] = 0xE9;
    *reinterpret_cast<int32_t*>(code + pos) = static_cast<int32_t>((found + replaceLen) - (code + pos + 4));
    pos += 4;

    DWORD oldProtect;
    VirtualProtect(found, replaceLen, PAGE_EXECUTE_READWRITE, &oldProtect);
    found[0] = 0xE9;
    *reinterpret_cast<int32_t*>(found + 1) = static_cast<int32_t>(code - (found + 5));
    for (int i = 5; i < replaceLen; ++i) found[i] = 0x90;
    VirtualProtect(found, replaceLen, oldProtect, &oldProtect);
    FlushInstructionCache(GetCurrentProcess(), found, replaceLen);
    return true;
}

static uint64_t g_fixLoDHits = 0;
extern "C" float __fastcall OnFixLoDCallback(float* rbxPtr, float originalVal) {
    g_fixLoDHits++;

    // Legitimate camera views have FOV > 30 degrees (e.g. 50, 75, 103 degrees).
    // Auxiliary views like shadow maps and reflections have 0.0f original FOV and
    // must NOT be overridden, otherwise culling for depth/reflections gets corrupted.
    float result = originalVal;
    if (originalVal > 30.0f) {
        // Default VR Mod behavior: Trick the LOD system into thinking the FOV is extremely narrow.
        // This prevents the engine from aggressively culling high-detail meshes under the feet.
        result = 3.04639287f; // Becomes 1.04719755f (approx 60 deg in radians) after mulss 0.34375f
    }

    if (g_fixLoDHits % 600 == 1) {
        Log("FixLoD: hits=%llu rbx=%p originalVal=%.6f result=%.6f\n",
            static_cast<unsigned long long>(g_fixLoDHits),
            rbxPtr,
            originalVal,
            result);
    }
    return result;
}

bool InstallFixLoDHook() {
    // Match VR Mod's CP2077FixLoD site. The short prefix occurs three times in
    // current Cyberpunk builds; the trailing mulss xmm0,xmm0 disambiguates it.
    const char* pattern =
        "\xF3\x0F\x10\x43\x20\xF3\x0F\x59\x05"
        "\x00\x00\x00\x00\xE8\x00\x00\x00\x00\xF3\x0F\x59\xC0";
    const char* mask = "xxxxxxxxx????x????xxxx";
    uint8_t* found = static_cast<uint8_t*>(FindPattern("Cyberpunk2077.exe", pattern, mask));
    if (!found) {
        Log("FixLoD hook: Pattern not found!\n");
        return false;
    }

    constexpr int replaceLen = 5; // movss (5)
    void* tramp = AllocateTrampoline(found, 256);
    if (!tramp) return false;

    uint8_t* code = static_cast<uint8_t*>(tramp);
    int pos = 0;

    // --- CALL C++ CALLBACK ---
    // Save volatile registers
    code[pos++] = 0x9C; // pushfq
    code[pos++] = 0x50; // push rax
    code[pos++] = 0x51; // push rcx
    code[pos++] = 0x52; // push rdx
    code[pos++] = 0x41; code[pos++] = 0x50; // push r8
    code[pos++] = 0x41; code[pos++] = 0x51; // push r9
    code[pos++] = 0x41; code[pos++] = 0x52; // push r10
    code[pos++] = 0x41; code[pos++] = 0x53; // push r11
    code[pos++] = 0x55; // push rbp

    // Save xmm registers
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x40; // sub rsp, 40h
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x04; code[pos++] = 0x24; // movups [rsp], xmm0
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10; // movups [rsp+10h], xmm1
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20; // movups [rsp+20h], xmm2
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30; // movups [rsp+30h], xmm3

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xE5; // mov rbp, rsp
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xE4; code[pos++] = 0xF0; // and rsp, -16
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x20; // sub rsp, 20h

    // Set arg1 (rcx) = rbx
    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xD9; // mov rcx, rbx
    // Set arg2 (xmm1) = [rbx + 20h]
    code[pos++] = 0xF3; code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x4B; code[pos++] = 0x20; // movss xmm1, [rbx+20h]

    WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(OnFixLoDCallback));
    code[pos++] = 0xFF; code[pos++] = 0xD0; // call rax

    // Save returned value (xmm0) to stack slot for xmm0
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x45; code[pos++] = 0x00; // movups [rbp], xmm0

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xEC; // mov rsp, rbp

    // Restore xmm registers (xmm0 will be our returned value!)
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x04; code[pos++] = 0x24; // movups xmm0, [rsp]
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10; // movups xmm1, [rsp+10h]
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20; // movups xmm2, [rsp+20h]
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30; // movups xmm3, [rsp+30h]
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xC4; code[pos++] = 0x40; // add rsp, 40h

    // Restore volatile registers
    code[pos++] = 0x5D; // pop rbp
    code[pos++] = 0x41; code[pos++] = 0x5B; // pop r11
    code[pos++] = 0x41; code[pos++] = 0x5A; // pop r10
    code[pos++] = 0x41; code[pos++] = 0x59; // pop r9
    code[pos++] = 0x41; code[pos++] = 0x58; // pop r8
    code[pos++] = 0x5A; // pop rdx
    code[pos++] = 0x59; // pop rcx
    code[pos++] = 0x58; // pop rax
    code[pos++] = 0x9D; // popfq

    // Jump back to found + 5
    code[pos++] = 0xE9;
    *reinterpret_cast<int32_t*>(code + pos) = static_cast<int32_t>((found + replaceLen) - (code + pos + 4));
    pos += 4;

    DWORD oldProtect;
    VirtualProtect(found, replaceLen, PAGE_EXECUTE_READWRITE, &oldProtect);
    found[0] = 0xE9;
    *reinterpret_cast<int32_t*>(found + 1) = static_cast<int32_t>(code - (found + 5));
    for (int i = 5; i < replaceLen; ++i) found[i] = 0x90; // NOP
    VirtualProtect(found, replaceLen, oldProtect, &oldProtect);
    FlushInstructionCache(GetCurrentProcess(), found, replaceLen);
    
    Log("FixLoD hook: Installed successfully at %p! Replaced 5 bytes with trampoline %p.\n", found, tramp);
    return true;
}

extern "C" void __fastcall OnMenuModeHookCallback(void* menuState, int newMode) {
    const int prevMode = g_menuModeValue;
    g_menuModeValue = newMode;

    if (prevMode != newMode) {
        if (g_verboseLog) Log("MenuMode hook: state=%p prev=%d new=%d\n", menuState, prevMode, newMode);
    }
}

bool InstallMenuModeHook() {
    const char* pattern = "\x33\xC9\x41\x8B\xD0\x41\x8B\xC0\x87\x83\x28\x01\x00\x00";
    const char* mask = "xxxxxxxxxxxxxx";
    uint8_t* found = static_cast<uint8_t*>(FindPattern("Cyberpunk2077.exe", pattern, mask));
    if (!found) return false;

    constexpr int replaceLen = 14;
    void* tramp = AllocateTrampoline(found, 384);
    if (!tramp) return false;

    uint8_t* code = static_cast<uint8_t*>(tramp);
    int pos = 0;

    code[pos++] = 0x9C;
    code[pos++] = 0x50;
    code[pos++] = 0x51;
    code[pos++] = 0x52;
    code[pos++] = 0x41; code[pos++] = 0x50;
    code[pos++] = 0x41; code[pos++] = 0x51;
    code[pos++] = 0x41; code[pos++] = 0x52;
    code[pos++] = 0x41; code[pos++] = 0x53;
    code[pos++] = 0x55;

    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x40; // sub rsp, 40h
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x04; code[pos++] = 0x24; // movups [rsp], xmm0
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10; // movups [rsp+10h], xmm1
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20; // movups [rsp+20h], xmm2
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30; // movups [rsp+30h], xmm3

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xE5; // mov rbp, rsp
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xE4; code[pos++] = 0xF0; // and rsp, -16
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x20;

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xD9; // mov rcx,rbx
    code[pos++] = 0x44; code[pos++] = 0x89; code[pos++] = 0xC2; // mov edx,r8d
    WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(OnMenuModeHookCallback));
    code[pos++] = 0xFF; code[pos++] = 0xD0;
    
    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xEC; // mov rsp, rbp

    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x04; code[pos++] = 0x24; // movups xmm0, [rsp]
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10; // movups xmm1, [rsp+10h]
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20; // movups xmm2, [rsp+20h]
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30; // movups xmm3, [rsp+30h]
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xC4; code[pos++] = 0x40; // add rsp, 40h

    code[pos++] = 0x5D;
    code[pos++] = 0x41; code[pos++] = 0x5B;
    code[pos++] = 0x41; code[pos++] = 0x5A;
    code[pos++] = 0x41; code[pos++] = 0x59;
    code[pos++] = 0x41; code[pos++] = 0x58;
    code[pos++] = 0x5A;
    code[pos++] = 0x59;
    code[pos++] = 0x58;
    code[pos++] = 0x9D;

    code[pos++] = 0x33; code[pos++] = 0xC9;
    code[pos++] = 0x41; code[pos++] = 0x8B; code[pos++] = 0xD0;
    code[pos++] = 0x41; code[pos++] = 0x8B; code[pos++] = 0xC0;
    code[pos++] = 0x87; code[pos++] = 0x83; code[pos++] = 0x28; code[pos++] = 0x01; code[pos++] = 0x00; code[pos++] = 0x00;

    code[pos++] = 0xE9;
    *reinterpret_cast<int32_t*>(code + pos) = static_cast<int32_t>((found + replaceLen) - (code + pos + 4));
    pos += 4;

    DWORD oldProtect;
    VirtualProtect(found, replaceLen, PAGE_EXECUTE_READWRITE, &oldProtect);
    found[0] = 0xE9;
    *reinterpret_cast<int32_t*>(found + 1) = static_cast<int32_t>(code - (found + 5));
    for (int i = 5; i < replaceLen; ++i) found[i] = 0x90;
    VirtualProtect(found, replaceLen, oldProtect, &oldProtect);
    FlushInstructionCache(GetCurrentProcess(), found, replaceLen);
    return true;
}

bool InstallForceHeadingUpdateHook() {
    const char* pattern = "\x48\x8B\xCB\xF3\x0F\x10\x4F\x1C\xE8\x00\x00\x00\x00\x84\xC0";
    const char* mask = "xxxxxxxxx????xx";
    uint8_t* found = static_cast<uint8_t*>(FindPattern("Cyberpunk2077.exe", pattern, mask));
    if (!found) return false;

    constexpr int replaceLen = 15;
    void* tramp = AllocateTrampoline(found, 256);
    if (!tramp) return false;

    const int32_t relCall = *reinterpret_cast<int32_t*>(found + 9);
    const uintptr_t callTarget = reinterpret_cast<uintptr_t>(found + 13) + relCall;

    uint8_t* code = static_cast<uint8_t*>(tramp);
    int pos = 0;

    code[pos++] = 0x48; code[pos++] = 0x8B; code[pos++] = 0xCB; // mov rcx,rbx
    code[pos++] = 0xF3; code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x4F; code[pos++] = 0x1C; // movss xmm1,[rdi+1C]
    WriteMovRaxImm64(code, pos, callTarget);
    code[pos++] = 0xFF; code[pos++] = 0xD0; // call rax
    code[pos++] = 0x30; code[pos++] = 0xC0; // xor al,al

    code[pos++] = 0xE9;
    *reinterpret_cast<int32_t*>(code + pos) = static_cast<int32_t>((found + replaceLen) - (code + pos + 4));
    pos += 4;

    DWORD oldProtect;
    VirtualProtect(found, replaceLen, PAGE_EXECUTE_READWRITE, &oldProtect);
    found[0] = 0xE9;
    *reinterpret_cast<int32_t*>(found + 1) = static_cast<int32_t>(code - (found + 5));
    for (int i = 5; i < replaceLen; ++i) found[i] = 0x90;
    VirtualProtect(found, replaceLen, oldProtect, &oldProtect);
    FlushInstructionCache(GetCurrentProcess(), found, replaceLen);
    Log("ForceHeadingUpdate hook installed at %p target=%p\n", found, reinterpret_cast<void*>(callTarget));
    return true;
}

// Snap-turn yaw delta (degrees) pushed by the XInput hook when the user flicks
// the right stick. Applied here in one frame to give a true instant snap (no
// stick-driven smooth rotation). Atomic 32-bit float via bit-cast through int.
static volatile LONG g_pendingSnapYawDeltaBits = 0;

// Index of the yaw float inside the delta buffer (default 1). Overridable via
// xr_snap_turn_yaw_index in vrport.ini for quick experimentation if [1] is wrong.
extern "C" int GetSnapTurnYawIndex();

extern "C" void __fastcall OnOnFootDeltaHeadCallback(float* deltaHead) {
    if (!deltaHead) return;
    const LONG bits = InterlockedExchange(&g_pendingSnapYawDeltaBits, 0);
    if (bits == 0) return;
    float snap = 0.0f;
    memcpy(&snap, &bits, sizeof(snap));
    if (snap == 0.0f) return;

    int idx = GetSnapTurnYawIndex();
    if (idx < 0) idx = 0;
    if (idx > 3) idx = 3;
    deltaHead[idx] += snap;
}

bool InstallOnFootDeltaHeadHook() {
    const char* pattern = "\xF3\x0F\x10\x81\x9C\x00\x00\x00\x48\x8D\x54\x24\x30";
    const char* mask = "xxxxxxxxxxxxx";
    uint8_t* found = static_cast<uint8_t*>(FindPattern("Cyberpunk2077.exe", pattern, mask));
    if (!found) return false;

    constexpr int replaceLen = 8; // movss xmm0,[rcx+9Ch]
    void* tramp = AllocateTrampoline(found, 512);
    if (!tramp) return false;

    uint8_t* code = static_cast<uint8_t*>(tramp);
    int pos = 0;

    // --- CALL C++ CALLBACK ---
    code[pos++] = 0x9C; // pushfq
    code[pos++] = 0x50; // push rax
    code[pos++] = 0x51; // push rcx
    code[pos++] = 0x52; // push rdx
    code[pos++] = 0x41; code[pos++] = 0x50; // push r8
    code[pos++] = 0x41; code[pos++] = 0x51; // push r9
    code[pos++] = 0x41; code[pos++] = 0x52; // push r10
    code[pos++] = 0x41; code[pos++] = 0x53; // push r11
    code[pos++] = 0x55; // push rbp

    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x40; // sub rsp, 40h
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x04; code[pos++] = 0x24; // movups [rsp+00h], xmm0
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10; // movups [rsp+10h], xmm1
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20; // movups [rsp+20h], xmm2
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30; // movups [rsp+30h], xmm3

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xE5; // mov rbp, rsp
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xE4; code[pos++] = 0xF0; // and rsp, -16
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x20; // sub rsp, 20h

    // Set arg1 (rcx) = rcx + 9Ch
    code[pos++] = 0x48; code[pos++] = 0x8D; code[pos++] = 0x89; code[pos++] = 0x9C; code[pos++] = 0x00; code[pos++] = 0x00; code[pos++] = 0x00; // lea rcx, [rcx+9Ch]

    WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(OnOnFootDeltaHeadCallback));
    code[pos++] = 0xFF; code[pos++] = 0xD0; // call rax

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xEC; // mov rsp, rbp

    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x04; code[pos++] = 0x24; // movups xmm0, [rsp+00h]
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10; // movups xmm1, [rsp+10h]
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20; // movups xmm2, [rsp+20h]
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30; // movups xmm3, [rsp+30h]
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xC4; code[pos++] = 0x40; // add rsp, 40h

    code[pos++] = 0x5D; // pop rbp
    code[pos++] = 0x41; code[pos++] = 0x5B; // pop r11
    code[pos++] = 0x41; code[pos++] = 0x5A; // pop r10
    code[pos++] = 0x41; code[pos++] = 0x59; // pop r9
    code[pos++] = 0x41; code[pos++] = 0x58; // pop r8
    code[pos++] = 0x5A; // pop rdx
    code[pos++] = 0x59; // pop rcx
    code[pos++] = 0x58; // pop rax
    code[pos++] = 0x9D; // popfq

    // Original instruction: movss xmm0, [rcx+9Ch]
    code[pos++] = 0xF3; code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x81;
    code[pos++] = 0x9C; code[pos++] = 0x00; code[pos++] = 0x00; code[pos++] = 0x00;

    // jmp back
    code[pos++] = 0xE9;
    *reinterpret_cast<int32_t*>(code + pos) = static_cast<int32_t>((found + replaceLen) - (code + pos + 4));
    pos += 4;

    DWORD oldProtect;
    VirtualProtect(found, replaceLen, PAGE_EXECUTE_READWRITE, &oldProtect);
    found[0] = 0xE9;
    *reinterpret_cast<int32_t*>(found + 1) = static_cast<int32_t>(code - (found + 5));
    for (int i = 5; i < replaceLen; ++i) found[i] = 0x90;
    VirtualProtect(found, replaceLen, oldProtect, &oldProtect);
    FlushInstructionCache(GetCurrentProcess(), found, replaceLen);
    return true;
}

static uint64_t g_patchBufferHits = 0;

extern "C" void __fastcall OnPatchBufferCallback(float* dest, float* src, size_t count) {
    g_patchBufferHits++;

    if (!OpenXRManager::Get().IsAERSubmitEnabled()) return;
    if (!dest || !src) return;
    if (count < 12 || count > 20) return;
    if ((g_patchBufferHits % 600) != 1) return;

    float srcValues[16] = {};
    float destValues[16] = {};
    const size_t sampleCount = count >= 16 ? 16 : count;
    if (!ReadFloatArraySafe(src, srcValues, sampleCount)) return;
    if (!ReadFloatArraySafe(dest, destValues, sampleCount)) return;

    Log("PatchBuffer probe: hit=%llu count=%zu dest=%p src=%p eye=%d\n",
        static_cast<unsigned long long>(g_patchBufferHits),
        count,
        dest,
        src,
        OpenXRManager::Get().GetCurrentRenderEyeIndex());

    if (sampleCount >= 16 && LooksProjectionLike(srcValues, sampleCount)) {
        LogMatrix4x4("PatchBuffer src projection candidate:", srcValues);
        LogMatrix4x4("PatchBuffer dest projection candidate:", destValues);
    } else {
        Log("PatchBuffer src[0..%zu]:", sampleCount - 1);
        for (size_t i = 0; i < sampleCount; ++i) {
            Log(" %0.6f", srcValues[i]);
        }
        Log("\n");
    }
}

bool InstallPatchBufferHook() {
    // 48 8B C1 4C 8D 15 ?? ?? ?? ?? 49 83 F8 0F
    const char* pattern = "\x48\x8B\xC1\x4C\x8D\x15\x00\x00\x00\x00\x49\x83\xF8\x0F";
    const char* mask = "xxxxxx????xxxx";
    uint8_t* found = static_cast<uint8_t*>(FindPattern("Cyberpunk2077.exe", pattern, mask));
    if (!found) return false;

    constexpr int replaceLen = 14; 
    void* tramp = AllocateTrampoline(found, 512);
    if (!tramp) return false;

    uint8_t* code = static_cast<uint8_t*>(tramp);
    int pos = 0;

    // --- CALL C++ CALLBACK ---
    code[pos++] = 0x9C; // pushfq
    code[pos++] = 0x50; // push rax
    code[pos++] = 0x51; // push rcx
    code[pos++] = 0x52; // push rdx
    code[pos++] = 0x41; code[pos++] = 0x50; // push r8
    code[pos++] = 0x41; code[pos++] = 0x51; // push r9
    code[pos++] = 0x41; code[pos++] = 0x52; // push r10
    code[pos++] = 0x41; code[pos++] = 0x53; // push r11
    code[pos++] = 0x55; // push rbp

    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x40; // sub rsp, 40h
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x04; code[pos++] = 0x24; // movups [rsp+00h], xmm0
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10; // movups [rsp+10h], xmm1
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20; // movups [rsp+20h], xmm2
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30; // movups [rsp+30h], xmm3

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xE5; // mov rbp, rsp
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xE4; code[pos++] = 0xF0; // and rsp, -16
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x20; // sub rsp, 20h

    // args: rcx=dest, rdx=src, r8=count (already set!)
    WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(OnPatchBufferCallback));
    code[pos++] = 0xFF; code[pos++] = 0xD0; // call rax

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xEC; // mov rsp, rbp

    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x04; code[pos++] = 0x24; // movups xmm0, [rsp+00h]
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10; // movups xmm1, [rsp+10h]
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20; // movups xmm2, [rsp+20h]
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30; // movups xmm3, [rsp+30h]
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xC4; code[pos++] = 0x40; // add rsp, 40h

    code[pos++] = 0x5D; // pop rbp
    code[pos++] = 0x41; code[pos++] = 0x5B; // pop r11
    code[pos++] = 0x41; code[pos++] = 0x5A; // pop r10
    code[pos++] = 0x41; code[pos++] = 0x59; // pop r9
    code[pos++] = 0x41; code[pos++] = 0x58; // pop r8
    code[pos++] = 0x5A; // pop rdx
    code[pos++] = 0x59; // pop rcx
    code[pos++] = 0x58; // pop rax
    code[pos++] = 0x9D; // popfq

    // Original instructions:
    // mov rax, rcx
    code[pos++] = 0x48; code[pos++] = 0x8B; code[pos++] = 0xC1;
    // lea r10, [rip+...]
    // We cannot copy RIP-relative LEA easily if it's pointing to something in the game executable.
    // found+3 is the start of lea r10, [rip+offset]. It's 7 bytes long.
    // The offset is *(int32_t*)(found+6).
    // Absolute address of target = (found + 3) + 7 + offset.
    int32_t leaOffset = *reinterpret_cast<int32_t*>(found + 6);
    uintptr_t targetAddr = reinterpret_cast<uintptr_t>(found) + 10 + leaOffset;
    
    // We can replace LEA with MOV R10, absolute_addr
    code[pos++] = 0x49; code[pos++] = 0xBA; // mov r10, imm64
    *reinterpret_cast<uint64_t*>(code + pos) = targetAddr;
    pos += 8;

    // cmp r8, 0Fh
    code[pos++] = 0x49; code[pos++] = 0x83; code[pos++] = 0xF8; code[pos++] = 0x0F;

    // jmp back
    code[pos++] = 0xE9;
    *reinterpret_cast<int32_t*>(code + pos) = static_cast<int32_t>((found + replaceLen) - (code + pos + 4));
    pos += 4;

    DWORD oldProtect;
    VirtualProtect(found, replaceLen, PAGE_EXECUTE_READWRITE, &oldProtect);
    found[0] = 0xE9;
    *reinterpret_cast<int32_t*>(found + 1) = static_cast<int32_t>(code - (found + 5));
    for (int i = 5; i < replaceLen; ++i) found[i] = 0x90;
    VirtualProtect(found, replaceLen, oldProtect, &oldProtect);
    FlushInstructionCache(GetCurrentProcess(), found, replaceLen);
    return true;
}

extern "C" void __fastcall OnSettingsResCallback(void* settingsPtr) {
    g_settingsResHits++;

    const uintptr_t settings = reinterpret_cast<uintptr_t>(settingsPtr);
    if (settings < 0x10000) {
        return;
    }

    const uintptr_t prev = g_settingsResPtr;
    g_settingsResPtr = settings;
    ApplySettingsResolutionOverride(settings);

    if (settings != prev || ((g_settingsResHits % 600) == 1)) {
        uint32_t activeWidth = 0;
        uint32_t activeHeight = 0;
        uint32_t targetWidth = 0;
        uint32_t targetHeight = 0;
        ReadU32Safe(settings + 0x18, &activeWidth);
        ReadU32Safe(settings + 0x1C, &activeHeight);
        ReadU32Safe(settings + 0x84, &targetWidth);
        ReadU32Safe(settings + 0x88, &targetHeight);
        Log("SettingsRes hook: ptr=%p active=%ux%u target=%ux%u forced=%ux%u\n",
            reinterpret_cast<void*>(settings),
            activeWidth,
            activeHeight,
            targetWidth,
            targetHeight,
            GetForcedRenderWidthValue(),
            GetForcedRenderHeightValue());
    }
}

bool InstallSettingsResHook() {
    const char* pattern = "\x39\x41\x18\x75\x00\x8B\x81\x88\x00\x00\x00\x39\x41\x1C";
    const char* mask = "xxxx?xxxxxxxxx";
    uint8_t* found = static_cast<uint8_t*>(FindPattern("Cyberpunk2077.exe", pattern, mask));
    if (!found) return false;

    constexpr int replaceLen = 14;
    void* tramp = AllocateTrampoline(found, 512);
    if (!tramp) return false;

    uint8_t* code = static_cast<uint8_t*>(tramp);
    int pos = 0;

    code[pos++] = 0x9C;
    code[pos++] = 0x50;
    code[pos++] = 0x51;
    code[pos++] = 0x52;
    code[pos++] = 0x41; code[pos++] = 0x50;
    code[pos++] = 0x41; code[pos++] = 0x51;
    code[pos++] = 0x41; code[pos++] = 0x52;
    code[pos++] = 0x41; code[pos++] = 0x53;
    code[pos++] = 0x55;

    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x40;
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x04; code[pos++] = 0x24;
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10;
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20;
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30;

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xE5;
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xE4; code[pos++] = 0xF0;
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x20;

    WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(OnSettingsResCallback));
    code[pos++] = 0xFF; code[pos++] = 0xD0;

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xEC;

    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x04; code[pos++] = 0x24;
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10;
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20;
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30;
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xC4; code[pos++] = 0x40;

    code[pos++] = 0x5D;
    code[pos++] = 0x41; code[pos++] = 0x5B;
    code[pos++] = 0x41; code[pos++] = 0x5A;
    code[pos++] = 0x41; code[pos++] = 0x59;
    code[pos++] = 0x41; code[pos++] = 0x58;
    code[pos++] = 0x5A;
    code[pos++] = 0x59;
    code[pos++] = 0x58;
    code[pos++] = 0x9D;

    code[pos++] = 0x39; code[pos++] = 0x41; code[pos++] = 0x18;
    code[pos++] = 0x75; code[pos++] = 0x06;
    code[pos++] = 0x8B; code[pos++] = 0x81; code[pos++] = 0x88; code[pos++] = 0x00; code[pos++] = 0x00; code[pos++] = 0x00;
    code[pos++] = 0x39; code[pos++] = 0x41; code[pos++] = 0x1C;

    code[pos++] = 0xE9;
    *reinterpret_cast<int32_t*>(code + pos) = static_cast<int32_t>((found + replaceLen) - (code + pos + 4));
    pos += 4;

    DWORD oldProtect;
    VirtualProtect(found, replaceLen, PAGE_EXECUTE_READWRITE, &oldProtect);
    found[0] = 0xE9;
    *reinterpret_cast<int32_t*>(found + 1) = static_cast<int32_t>(code - (found + 5));
    for (int i = 5; i < replaceLen; ++i) found[i] = 0x90;
    VirtualProtect(found, replaceLen, oldProtect, &oldProtect);
    FlushInstructionCache(GetCurrentProcess(), found, replaceLen);
    return true;
}

extern "C" void __fastcall OnDLSSResCallback(void* dlssPtr) {
    g_dlssResHits++;

    const uintptr_t dlss = reinterpret_cast<uintptr_t>(dlssPtr);
    if (dlss < 0x10000) {
        return;
    }

    const uintptr_t prev = g_dlssResPtr;
    g_dlssResPtr = dlss;
    ApplyDLSSResolutionOverride(dlss);

    if (dlss != prev || ((g_dlssResHits % 600) == 1)) {
        uint32_t sizeA = 0;
        uint32_t sizeB = 0;
        uint32_t sizeC = 0;
        ReadU32Safe(dlss + 0x04, &sizeA);
        ReadU32Safe(dlss + 0x18, &sizeB);
        ReadU32Safe(dlss + 0x1C, &sizeC);
        Log("DLSSRes hook: ptr=%p fields=%u/%u/%u forcedSquare=%u\n",
            reinterpret_cast<void*>(dlss),
            sizeA,
            sizeB,
            sizeC,
            GetForcedSquareResolutionValue());
    }
}

bool InstallDLSSResHook() {
    const char* pattern = "\x4C\x89\x73\x30\x89\x43\x04\x89\x43\x18\x89\x43\x1C";
    const char* mask = "xxxxxxxxxxxxx";
    uint8_t* found = static_cast<uint8_t*>(FindPattern("Cyberpunk2077.exe", pattern, mask));
    if (!found) return false;

    constexpr int replaceLen = 13;
    void* tramp = AllocateTrampoline(found, 512);
    if (!tramp) return false;

    uint8_t* code = static_cast<uint8_t*>(tramp);
    int pos = 0;

    code[pos++] = 0x4C; code[pos++] = 0x89; code[pos++] = 0x73; code[pos++] = 0x30;
    code[pos++] = 0x89; code[pos++] = 0x43; code[pos++] = 0x04;
    code[pos++] = 0x89; code[pos++] = 0x43; code[pos++] = 0x18;
    code[pos++] = 0x89; code[pos++] = 0x43; code[pos++] = 0x1C;

    code[pos++] = 0x9C;
    code[pos++] = 0x50;
    code[pos++] = 0x51;
    code[pos++] = 0x52;
    code[pos++] = 0x41; code[pos++] = 0x50;
    code[pos++] = 0x41; code[pos++] = 0x51;
    code[pos++] = 0x41; code[pos++] = 0x52;
    code[pos++] = 0x41; code[pos++] = 0x53;
    code[pos++] = 0x55;

    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x40;
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x04; code[pos++] = 0x24;
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10;
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20;
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30;

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xE5;
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xE4; code[pos++] = 0xF0;
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x20;

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xD9;
    WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(OnDLSSResCallback));
    code[pos++] = 0xFF; code[pos++] = 0xD0;

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xEC;

    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x04; code[pos++] = 0x24;
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10;
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20;
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30;
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xC4; code[pos++] = 0x40;

    code[pos++] = 0x5D;
    code[pos++] = 0x41; code[pos++] = 0x5B;
    code[pos++] = 0x41; code[pos++] = 0x5A;
    code[pos++] = 0x41; code[pos++] = 0x59;
    code[pos++] = 0x41; code[pos++] = 0x58;
    code[pos++] = 0x5A;
    code[pos++] = 0x59;
    code[pos++] = 0x58;
    code[pos++] = 0x9D;

    code[pos++] = 0xE9;
    *reinterpret_cast<int32_t*>(code + pos) = static_cast<int32_t>((found + replaceLen) - (code + pos + 4));
    pos += 4;

    DWORD oldProtect;
    VirtualProtect(found, replaceLen, PAGE_EXECUTE_READWRITE, &oldProtect);
    found[0] = 0xE9;
    *reinterpret_cast<int32_t*>(found + 1) = static_cast<int32_t>(code - (found + 5));
    for (int i = 5; i < replaceLen; ++i) found[i] = 0x90;
    VirtualProtect(found, replaceLen, oldProtect, &oldProtect);
    FlushInstructionCache(GetCurrentProcess(), found, replaceLen);
    return true;
}

static void LogDLSSMatricesStateWindow(uintptr_t state) {
    if (!state) return;

    Log("DLSSMatrices state fields near +0x1A0:\n");
    for (int offset = 0x180; offset <= 0x1B0; offset += 4) {
        char label[64];
        sprintf_s(label, "dlssMatrix state+0x%03X", offset);
        LogU32FloatAt(label, state + static_cast<uintptr_t>(offset));
    }

    LogVec4At("dlssMatrix state+0x140", state + 0x140);
    LogVec4At("dlssMatrix state+0x160", state + 0x160);
    LogVec4At("dlssMatrix state+0x180", state + 0x180);
    LogVec4At("dlssMatrix state+0x1A0", state + 0x1A0);

    if (!IsReadableAddressRange(state, 0x260)) {
        Log("DLSSMatrices matrix scan skipped: state range is not fully readable.\n");
        return;
    }

    int candidates = 0;
    for (int offset = 0; offset <= 0x200; offset += 0x10) {
        float values[16] = {};
        if (!ReadFloatArraySafe(reinterpret_cast<const float*>(state + static_cast<uintptr_t>(offset)), values, 16)) {
            continue;
        }
        if (!LooksProjectionLike(values, 16)) {
            continue;
        }

        char prefix[96];
        sprintf_s(prefix, "DLSSMatrices projection-like candidate state+0x%03X:", offset);
        LogMatrix4x4(prefix, values);
        if (++candidates >= 4) {
            break;
        }
    }

    if (candidates == 0) {
        Log("DLSSMatrices: no projection-like 4x4 found in state+0x000..0x200.\n");
    }
}

extern "C" uint32_t __fastcall OnDLSSMatricesCallback(void* callThis, void* matrixState, uint32_t matrixSlot) {
    uint32_t adjustedSlot = matrixSlot;
    const int eye = OpenXRManager::Get().GetCurrentRenderEyeIndex();

    if (g_liveControls.xrDLSSMatrixHook != 0) {
        switch (g_liveControls.xrDLSSSlotMode) {
        case 1:
            // L/R frames from the same AER pair receive the same temporal index.
            adjustedSlot = matrixSlot >> 1;
            break;
        case 2:
            // Keep two stable slots, one per eye. This is a diagnostic history-isolation mode.
            adjustedSlot = eye > 0 ? 1u : 0u;
            break;
        case 3:
            // Freeze the DLSS matrix index to test whether temporal accumulation is the ghost source.
            adjustedSlot = 0;
            break;
        default:
            adjustedSlot = matrixSlot;
            break;
        }
    }
    g_dlssMatricesAdjustedSlot = adjustedSlot;

    if (g_liveControls.xrDLSSMatrixHook == 0) {
        return adjustedSlot;
    }

    const uint64_t hit = ++g_dlssMatricesHits;
    const uintptr_t callThisAddr = reinterpret_cast<uintptr_t>(callThis);
    const uintptr_t state = reinterpret_cast<uintptr_t>(matrixState);

    g_dlssMatricesThis = callThisAddr;
    g_dlssMatricesState = state;
    g_dlssMatricesSlot = matrixSlot;
    g_dlssMatricesEye = eye;

    const int logStride = g_liveControls.xrDLSSLogStride;
    const bool periodicLog = logStride > 0 && (hit % static_cast<uint64_t>(logStride)) == 1;
    if (hit > 8 && !periodicLog) {
        return adjustedSlot;
    }

    const uintptr_t returnAddr = g_dlssMatricesHookSite ? g_dlssMatricesHookSite + 14 : 0;
    const unsigned long long siteRva = IsInGameModule(g_dlssMatricesHookSite)
        ? static_cast<unsigned long long>(g_dlssMatricesHookSite - g_gameModuleBase)
        : 0ULL;
    const unsigned long long targetRva = IsInGameModule(g_dlssMatricesCallTarget)
        ? static_cast<unsigned long long>(g_dlssMatricesCallTarget - g_gameModuleBase)
        : 0ULL;

    if (g_verboseLog) Log("DLSSMatrices hook: hit=%llu site=%p(rva=0x%llX) target=%p(rva=0x%llX) return=%p this=%p state=%p slot=%u adjusted=%u eye=%d mode=%d\n",
        static_cast<unsigned long long>(hit),
        reinterpret_cast<void*>(g_dlssMatricesHookSite),
        siteRva,
        reinterpret_cast<void*>(g_dlssMatricesCallTarget),
        targetRva,
        reinterpret_cast<void*>(returnAddr),
        reinterpret_cast<void*>(callThisAddr),
        reinterpret_cast<void*>(state),
        matrixSlot,
        adjustedSlot,
        g_dlssMatricesEye,
        g_liveControls.xrDLSSSlotMode);

    if (state < 0x10000) {
        Log("DLSSMatrices state skipped: pointer is not plausible.\n");
        return adjustedSlot;
    }

    LogDLSSMatricesStateWindow(state);
    return adjustedSlot;
}

bool InstallDLSSMatricesHook() {
    const char* pattern = "\x48\x8B\xCB\x8B\x90\xA0\x01\x00\x00\xE8\x00\x00\x00\x00";
    const char* mask = "xxxxxxxxxx????";
    uint8_t* found = static_cast<uint8_t*>(FindPattern("Cyberpunk2077.exe", pattern, mask));
    if (!found) return false;

    constexpr int replaceLen = 14;
    const int32_t relCall = *reinterpret_cast<int32_t*>(found + 10);
    const uintptr_t callTarget = reinterpret_cast<uintptr_t>(found + replaceLen) + relCall;

    void* tramp = AllocateTrampoline(found, 512);
    if (!tramp) return false;

    g_dlssMatricesHookSite = reinterpret_cast<uintptr_t>(found);
    g_dlssMatricesCallTarget = callTarget;

    uint8_t* code = static_cast<uint8_t*>(tramp);
    int pos = 0;

    // Original setup for the DLSS matrices call.
    code[pos++] = 0x48; code[pos++] = 0x8B; code[pos++] = 0xCB; // mov rcx,rbx
    code[pos++] = 0x8B; code[pos++] = 0x90; code[pos++] = 0xA0; code[pos++] = 0x01;
    code[pos++] = 0x00; code[pos++] = 0x00; // mov edx,[rax+1A0h]

    code[pos++] = 0x9C;
    code[pos++] = 0x50;
    code[pos++] = 0x51;
    code[pos++] = 0x52;
    code[pos++] = 0x41; code[pos++] = 0x50;
    code[pos++] = 0x41; code[pos++] = 0x51;
    code[pos++] = 0x41; code[pos++] = 0x52;
    code[pos++] = 0x41; code[pos++] = 0x53;
    code[pos++] = 0x55;

    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x40;
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x04; code[pos++] = 0x24;
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10;
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20;
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30;

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xE5;
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xE4; code[pos++] = 0xF0;
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x20;

    code[pos++] = 0x41; code[pos++] = 0x89; code[pos++] = 0xD0; // mov r8d,edx
    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xC2; // mov rdx,rax
    WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(OnDLSSMatricesCallback));
    code[pos++] = 0xFF; code[pos++] = 0xD0;

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xEC;

    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x04; code[pos++] = 0x24;
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10;
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20;
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30;
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xC4; code[pos++] = 0x40;

    // OVERWRITE the saved 'rdx' on the stack with our returned 'eax' (adjustedSlot)
    // so that the upcoming 'pop rdx' puts it into the game's register!
    // rdx is located at [rsp+28h] after the above add.
    code[pos++] = 0x89; code[pos++] = 0x44; code[pos++] = 0x24; code[pos++] = 0x28; // mov dword ptr [rsp+28h], eax

    code[pos++] = 0x5D;
    code[pos++] = 0x41; code[pos++] = 0x5B;
    code[pos++] = 0x41; code[pos++] = 0x5A;
    code[pos++] = 0x41; code[pos++] = 0x59;
    code[pos++] = 0x41; code[pos++] = 0x58;
    code[pos++] = 0x5A;
    code[pos++] = 0x59;
    code[pos++] = 0x58;
    code[pos++] = 0x9D;

    WriteMovR11Imm64(code, pos, reinterpret_cast<uintptr_t>(&g_dlssMatricesAdjustedSlot));
    code[pos++] = 0x41; code[pos++] = 0x8B; code[pos++] = 0x13; // mov edx,[r11]

    // Preserve the original direct-call return address while jumping through an absolute target.
    WriteMovR11Imm64(code, pos, reinterpret_cast<uintptr_t>(found + replaceLen));
    code[pos++] = 0x41; code[pos++] = 0x53; // push r11
    WriteMovR11Imm64(code, pos, callTarget);
    code[pos++] = 0x41; code[pos++] = 0xFF; code[pos++] = 0xE3; // jmp r11

    DWORD oldProtect;
    VirtualProtect(found, replaceLen, PAGE_EXECUTE_READWRITE, &oldProtect);
    found[0] = 0xE9;
    *reinterpret_cast<int32_t*>(found + 1) = static_cast<int32_t>(code - (found + 5));
    for (int i = 5; i < replaceLen; ++i) found[i] = 0x90;
    VirtualProtect(found, replaceLen, oldProtect, &oldProtect);
    FlushInstructionCache(GetCurrentProcess(), found, replaceLen);

    Log("DLSSMatrices hook installed at %p target=%p\n", found, reinterpret_cast<void*>(callTarget));
    return true;
}

bool InstallFreeDeltaHeadHook() {
    const char* pattern = "\xF3\x0F\x11\x9E\xCC\x0C\x00\x00\x74\x5C";
    const char* mask = "xxxxxxxxxx";
    uint8_t* found = static_cast<uint8_t*>(FindPattern("Cyberpunk2077.exe", pattern, mask));
    if (!found) return false;

    constexpr int replaceLen = 8; // movss [rsi+0CCCh],xmm3
    void* tramp = AllocateTrampoline(found, 256);
    if (!tramp) return false;

    uint8_t* code = static_cast<uint8_t*>(tramp);
    int pos = 0;

    // Original instruction first. No live modification here: this path crashed when we
    // rewrote the scalar, so keep it telemetry-only.
    code[pos++] = 0xF3; code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x9E;
    code[pos++] = 0xCC; code[pos++] = 0x0C; code[pos++] = 0x00; code[pos++] = 0x00;

    // Telemetry after the original store.
    code[pos++] = 0x50;
    WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(g_telemetry) + kFreeDeltaTelemetryOffset);
    code[pos++] = 0xFF; code[pos++] = 0x00;
    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0x70; code[pos++] = 0x08;
    code[pos++] = 0xF3; code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x58; code[pos++] = 0x10; // movss [rax+10h],xmm3
    code[pos++] = 0x58;

    code[pos++] = 0xE9;
    *reinterpret_cast<int32_t*>(code + pos) = static_cast<int32_t>((found + replaceLen) - (code + pos + 4));
    pos += 4;

    DWORD oldProtect;
    VirtualProtect(found, replaceLen, PAGE_EXECUTE_READWRITE, &oldProtect);
    found[0] = 0xE9;
    *reinterpret_cast<int32_t*>(found + 1) = static_cast<int32_t>(code - (found + 5));
    for (int i = 5; i < replaceLen; ++i) found[i] = 0x90;
    VirtualProtect(found, replaceLen, oldProtect, &oldProtect);
    FlushInstructionCache(GetCurrentProcess(), found, replaceLen);
    return true;
}

// Head-oriented locomotion: rotate the on-foot move vector by the HMD yaw so
// "forward" follows the headset. moveStruct = rsi; [+0x90]=X (strafe), [+0x94]=Y
// (forward). Only active in HMD movement mode and outside menus; the vehicle path
// never hits OnFootMoveXY so driving is untouched.
extern "C" void OnOnFootMoveXYCallback(void* moveStruct) {
    const int src = g_liveControls.xrMovementSource;
    if (src <= 0) return; // 0 = Game (no rotation)
    if (g_menuModeValue != 0) return;
    if (!moveStruct) return;
    float* p = reinterpret_cast<float*>(reinterpret_cast<uint8_t*>(moveStruct) + 0x90);
    float x = p[0];
    float y = p[1];
    if (x == 0.0f && y == 0.0f) return;
    float yaw = 0.0f;
    switch (src) {
        case 1: yaw = OpenXRManager::Get().GetHmdYawRelToBody(); break;
        case 2: yaw = OpenXRManager::Get().GetHandYawRelToBody(0); break;
        case 3: yaw = OpenXRManager::Get().GetHandYawRelToBody(1); break;
        default: return;
    }
    float c = cosf(yaw);
    float s = sinf(yaw);
    p[0] = x * c - y * s;
    p[1] = x * s + y * c;
}

bool InstallOnFootMoveXYHook() {
    const char* pattern = "\xF3\x0F\x11\x86\x94\x00\x00\x00\xF3\x0F\x58\xCA";
    const char* mask = "xxxxxxxxxxxx";
    uint8_t* found = static_cast<uint8_t*>(FindPattern("Cyberpunk2077.exe", pattern, mask));
    if (!found) return false;

    constexpr int replaceLen = 8; // movss [rsi+94h],xmm0
    void* tramp = AllocateTrampoline(found, 256);
    if (!tramp) return false;

    uint8_t* code = static_cast<uint8_t*>(tramp);
    int pos = 0;

    // Original instruction first so the struct already has the new Y scalar.
    code[pos++] = 0xF3; code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x86;
    code[pos++] = 0x94; code[pos++] = 0x00; code[pos++] = 0x00; code[pos++] = 0x00;

    // Save volatile state (flags, GPRs, xmm0-5) before calling the C++ callback,
    // so the game's following 'addss xmm1,xmm2' and registers survive.
    code[pos++] = 0x9C;                                   // pushfq
    code[pos++] = 0x50;                                   // push rax
    code[pos++] = 0x51;                                   // push rcx
    code[pos++] = 0x52;                                   // push rdx
    code[pos++] = 0x41; code[pos++] = 0x50;              // push r8
    code[pos++] = 0x41; code[pos++] = 0x51;              // push r9
    code[pos++] = 0x41; code[pos++] = 0x52;              // push r10
    code[pos++] = 0x41; code[pos++] = 0x53;              // push r11
    code[pos++] = 0x55;                                   // push rbp

    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x60; // sub rsp,0x60
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x04; code[pos++] = 0x24;             // movups [rsp],xmm0
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10; // [rsp+10h],xmm1
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20; // xmm2
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30; // xmm3
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x64; code[pos++] = 0x24; code[pos++] = 0x40; // xmm4
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x6C; code[pos++] = 0x24; code[pos++] = 0x50; // xmm5

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xE5;             // mov rbp,rsp
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xE4; code[pos++] = 0xF0; // and rsp,-16
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x20; // sub rsp,0x20 (shadow)

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xF1;             // mov rcx,rsi (arg0)
    WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(OnOnFootMoveXYCallback));
    code[pos++] = 0xFF; code[pos++] = 0xD0;                                 // call rax

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xEC;             // mov rsp,rbp

    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x04; code[pos++] = 0x24;             // movups xmm0,[rsp]
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10;
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20;
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30;
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x64; code[pos++] = 0x24; code[pos++] = 0x40;
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x6C; code[pos++] = 0x24; code[pos++] = 0x50;
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xC4; code[pos++] = 0x60; // add rsp,0x60

    code[pos++] = 0x5D;                                   // pop rbp
    code[pos++] = 0x41; code[pos++] = 0x5B;              // pop r11
    code[pos++] = 0x41; code[pos++] = 0x5A;              // pop r10
    code[pos++] = 0x41; code[pos++] = 0x59;              // pop r9
    code[pos++] = 0x41; code[pos++] = 0x58;              // pop r8
    code[pos++] = 0x5A;                                   // pop rdx
    code[pos++] = 0x59;                                   // pop rcx
    code[pos++] = 0x58;                                   // pop rax
    code[pos++] = 0x9D;                                   // popfq

    code[pos++] = 0xE9;
    *reinterpret_cast<int32_t*>(code + pos) = static_cast<int32_t>((found + replaceLen) - (code + pos + 4));
    pos += 4;

    DWORD oldProtect;
    VirtualProtect(found, replaceLen, PAGE_EXECUTE_READWRITE, &oldProtect);
    found[0] = 0xE9;
    *reinterpret_cast<int32_t*>(found + 1) = static_cast<int32_t>(code - (found + 5));
    for (int i = 5; i < replaceLen; ++i) found[i] = 0x90;
    VirtualProtect(found, replaceLen, oldProtect, &oldProtect);
    FlushInstructionCache(GetCurrentProcess(), found, replaceLen);
    return true;
}

bool InstallNativeSetterMetaWriteHook() {
    const char* pattern = "\x48\x8B\x07\x48\x89\x4F\x08\x48\x8D\x4D\xE7\x48\x89\x45\xE7";
    const char* mask = "xxxxxxxxxxxxxxx";
    uint8_t* found = static_cast<uint8_t*>(FindPattern("Cyberpunk2077.exe", pattern, mask));
    if (!found) return false;

    constexpr int replaceLen = 11; // mov rax,[rdi] / mov [rdi+08],rcx / lea rcx,[rbp-19]
    void* tramp = AllocateTrampoline(found, 256);
    if (!tramp) return false;

    uint8_t* code = static_cast<uint8_t*>(tramp);
    int pos = 0;

    code[pos++] = 0x50; // push rax
    code[pos++] = 0x41; code[pos++] = 0x53; // push r11
    WriteMovR11Imm64(code, pos, reinterpret_cast<uintptr_t>(g_setterTrace) + kMetaWriteTraceOffset);
    code[pos++] = 0x48; code[pos++] = 0x8D; code[pos++] = 0x44; code[pos++] = 0x24; code[pos++] = 0x10; // lea rax,[rsp+10h]
    code[pos++] = 0x41; code[pos++] = 0xFF; code[pos++] = 0x03; // inc dword ptr [r11]
    code[pos++] = 0x49; code[pos++] = 0x89; code[pos++] = 0x7B; code[pos++] = 0x08; // mov [r11+08],rdi
    code[pos++] = 0x49; code[pos++] = 0x89; code[pos++] = 0x4B; code[pos++] = 0x10; // mov [r11+10],rcx
    code[pos++] = 0x49; code[pos++] = 0x89; code[pos++] = 0x43; code[pos++] = 0x18; // mov [r11+18],rax
    code[pos++] = 0x41; code[pos++] = 0x5B; // pop r11
    code[pos++] = 0x58; // pop rax

    code[pos++] = 0x48; code[pos++] = 0x8B; code[pos++] = 0x07; // mov rax,[rdi]
    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0x4F; code[pos++] = 0x08; // mov [rdi+08],rcx
    code[pos++] = 0x48; code[pos++] = 0x8D; code[pos++] = 0x4D; code[pos++] = 0xE7; // lea rcx,[rbp-19]

    code[pos++] = 0xE9;
    *reinterpret_cast<int32_t*>(code + pos) = static_cast<int32_t>((found + replaceLen) - (code + pos + 4));
    pos += 4;

    DWORD oldProtect;
    VirtualProtect(found, replaceLen, PAGE_EXECUTE_READWRITE, &oldProtect);
    found[0] = 0xE9;
    *reinterpret_cast<int32_t*>(found + 1) = static_cast<int32_t>(code - (found + 5));
    for (int i = 5; i < replaceLen; ++i) found[i] = 0x90;
    VirtualProtect(found, replaceLen, oldProtect, &oldProtect);
    FlushInstructionCache(GetCurrentProcess(), found, replaceLen);
    return true;
}

bool InstallNativeSetterMetaConsumeHook() {
    const char* pattern = "\x48\x8B\x42\x08\x48\x8D\x4C\x24\x20\x48\x83\x62\x08\x00\x48\x89\x44\x24\x28\x48\x8B\x02";
    const char* mask = "xxxxxxxxxxxxxxxxxxxxxx";
    uint8_t* found = static_cast<uint8_t*>(FindPattern("Cyberpunk2077.exe", pattern, mask));
    if (!found) return false;

    constexpr int replaceLen = 9; // mov rax,[rdx+08] / lea rcx,[rsp+20]
    void* tramp = AllocateTrampoline(found, 256);
    if (!tramp) return false;

    uint8_t* code = static_cast<uint8_t*>(tramp);
    int pos = 0;

    code[pos++] = 0x48; code[pos++] = 0x8B; code[pos++] = 0x42; code[pos++] = 0x08; // mov rax,[rdx+08]
    code[pos++] = 0x48; code[pos++] = 0x8D; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x20; // lea rcx,[rsp+20]

    code[pos++] = 0x50; // push rax
    code[pos++] = 0x41; code[pos++] = 0x53; // push r11
    WriteMovR11Imm64(code, pos, reinterpret_cast<uintptr_t>(g_setterTrace) + kMetaConsumeTraceOffset);
    code[pos++] = 0x48; code[pos++] = 0x8D; code[pos++] = 0x44; code[pos++] = 0x24; code[pos++] = 0x10; // lea rax,[rsp+10h]
    code[pos++] = 0x41; code[pos++] = 0xFF; code[pos++] = 0x03; // inc dword ptr [r11]
    code[pos++] = 0x49; code[pos++] = 0x89; code[pos++] = 0x53; code[pos++] = 0x08; // mov [r11+08],rdx
    code[pos++] = 0x49; code[pos++] = 0x89; code[pos++] = 0x43; code[pos++] = 0x18; // mov [r11+18],rax
    code[pos++] = 0x48; code[pos++] = 0x8B; code[pos++] = 0x44; code[pos++] = 0x24; code[pos++] = 0x08; // mov rax,[rsp+8]
    code[pos++] = 0x49; code[pos++] = 0x89; code[pos++] = 0x43; code[pos++] = 0x10; // mov [r11+10],rax
    code[pos++] = 0x41; code[pos++] = 0x5B; // pop r11
    code[pos++] = 0x58; // pop rax

    code[pos++] = 0xE9;
    *reinterpret_cast<int32_t*>(code + pos) = static_cast<int32_t>((found + replaceLen) - (code + pos + 4));
    pos += 4;

    DWORD oldProtect;
    VirtualProtect(found, replaceLen, PAGE_EXECUTE_READWRITE, &oldProtect);
    found[0] = 0xE9;
    *reinterpret_cast<int32_t*>(found + 1) = static_cast<int32_t>(code - (found + 5));
    for (int i = 5; i < replaceLen; ++i) found[i] = 0x90;
    VirtualProtect(found, replaceLen, oldProtect, &oldProtect);
    FlushInstructionCache(GetCurrentProcess(), found, replaceLen);
    return true;
}

bool InstallNativeSetterClearHook() {
    const char* pattern = "\xCC\x48\x83\x22\x00\x48\x83\x62\x08\x00\xC3\xCC";
    const char* mask = "xxxxxxxxxxxx";
    uint8_t* found = static_cast<uint8_t*>(FindPattern("Cyberpunk2077.exe", pattern, mask));
    if (!found) return false;

    uint8_t* hookAt = found + 1;
    constexpr int replaceLen = 9; // and qword ptr [rdx],00 / and qword ptr [rdx+08],00
    void* tramp = AllocateTrampoline(hookAt, 256);
    if (!tramp) return false;

    uint8_t* code = static_cast<uint8_t*>(tramp);
    int pos = 0;

    code[pos++] = 0x50; // push rax
    code[pos++] = 0x41; code[pos++] = 0x53; // push r11
    WriteMovR11Imm64(code, pos, reinterpret_cast<uintptr_t>(g_setterTrace) + kClearTraceOffset);
    code[pos++] = 0x41; code[pos++] = 0xFF; code[pos++] = 0x03; // inc dword ptr [r11]
    code[pos++] = 0x49; code[pos++] = 0x89; code[pos++] = 0x53; code[pos++] = 0x08; // mov [r11+08],rdx
    code[pos++] = 0x48; code[pos++] = 0x8B; code[pos++] = 0x44; code[pos++] = 0x24; code[pos++] = 0x10; // mov rax,[rsp+10h]
    code[pos++] = 0x49; code[pos++] = 0x89; code[pos++] = 0x43; code[pos++] = 0x10; // mov [r11+10],rax
    code[pos++] = 0x41; code[pos++] = 0x5B; // pop r11
    code[pos++] = 0x58; // pop rax

    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0x22; code[pos++] = 0x00; // and qword ptr [rdx],00
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0x62; code[pos++] = 0x08; code[pos++] = 0x00; // and qword ptr [rdx+08],00

    code[pos++] = 0xE9;
    *reinterpret_cast<int32_t*>(code + pos) = static_cast<int32_t>((hookAt + replaceLen) - (code + pos + 4));
    pos += 4;

    DWORD oldProtect;
    VirtualProtect(hookAt, replaceLen, PAGE_EXECUTE_READWRITE, &oldProtect);
    hookAt[0] = 0xE9;
    *reinterpret_cast<int32_t*>(hookAt + 1) = static_cast<int32_t>(code - (hookAt + 5));
    for (int i = 5; i < replaceLen; ++i) hookAt[i] = 0x90;
    VirtualProtect(hookAt, replaceLen, oldProtect, &oldProtect);
    FlushInstructionCache(GetCurrentProcess(), hookAt, replaceLen);
    return true;
}

// ===========================================================================
// XInput merge: hook XInputGetState in the loaded XInput1_*.dll and OR the VR
// controller snapshot from OpenXRManager into the gamepad state the game reads
// every frame. CP2077 already has full native gamepad bindings so movement,
// jump, dodge, fire/aim, weapon switch, reload, etc. all "just work" once the
// VR state lands in XINPUT_GAMEPAD.
// ===========================================================================

using XInputGetState_t = DWORD (WINAPI*)(DWORD, XINPUT_STATE*);

static XInputGetState_t g_realXInputGetState = nullptr;
static uint8_t  g_xinputOrig[16] = {};
static uint8_t* g_xinputHookAt = nullptr;
static bool     g_xinputHooked = false;
static int      g_xinputSnapArmedDir = 0;       // currently latched stick direction so we don't fire while held
static DWORD    g_xinputSnapPulseStartMs = 0;   // when the current pulse began
static int      g_xinputSnapPulseDir = 0;       // direction of the active pulse (0 = idle)

extern "C" int GetSnapTurnPulseMs();

static SHORT FloatToSHORT(float v) {
    if (v > 1.0f) v = 1.0f;
    if (v < -1.0f) v = -1.0f;
    return (v >= 0.0f) ? static_cast<SHORT>(v * 32767.0f) : static_cast<SHORT>(v * 32768.0f);
}
static BYTE FloatToBYTE(float v) {
    if (v < 0.0f) v = 0.0f;
    if (v > 1.0f) v = 1.0f;
    return static_cast<BYTE>(v * 255.0f);
}
static float ApplyStickDeadzone(float v, float dz) {
    float a = v < 0.0f ? -v : v;
    if (a < dz) return 0.0f;
    float s = v < 0.0f ? -1.0f : 1.0f;
    return s * (a - dz) / (1.0f - dz);
}

static DWORD WINAPI HookedXInputGetState(DWORD dwUserIndex, XINPUT_STATE* pState) {
    DWORD r = ERROR_DEVICE_NOT_CONNECTED;
    if (g_realXInputGetState) r = g_realXInputGetState(dwUserIndex, pState);

    if (!pState) return r;
    if (dwUserIndex != 0) return r;
    if (g_liveControls.xrXInputHook == 0) return r;

    VRControllerState vr{};
    if (!OpenXRManager::Get().GetControllerState(&vr)) return r;

    if (r != ERROR_SUCCESS) {
        memset(pState, 0, sizeof(*pState));
        r = ERROR_SUCCESS;
    }

    // Buttons: OR (so a physical pad can still augment, and vice versa).
    pState->Gamepad.wButtons |= vr.buttons;

    // Triggers: take the max so a physical squeeze isn't lost.
    BYTE lt = FloatToBYTE(vr.leftTrigger);
    BYTE rt = FloatToBYTE(vr.rightTrigger);
    if (lt > pState->Gamepad.bLeftTrigger)  pState->Gamepad.bLeftTrigger  = lt;

    // Publish whether the VR right trigger is held (shared[30]) so the CET melee mod can use it as the
    // power/strong modifier (hold = strong attack).
    OpenXRManager::Get().SetSharedSlot(30, (vr.rightTrigger > 0.5f) ? 1.0f : 0.0f);
    // Right grip is published as a BINARY pressed/not-pressed flag in shared[49] (a previously free
    // slot — [50]/[51]/[52] are owned by the camera-trace producer and we were stomping on them).
    // The CET hand-to-holster mod reads this + the IN-GAME wrist-to-hip distances the plugin
    // publishes from the live FK pose to decide whether reaching for a visual holster + a grip
    // press should equip / unequip the corresponding weapon.
    OpenXRManager::Get().SetSharedSlot(49, vr.rightGrip > 0.5f ? 1.0f : 0.0f);
    // Melee RT IMPULSE (shared[29] = a frame countdown the CET mod raises on a detected VR swing): tap RT
    // so the game enters its NATIVE melee-attack state (full native damage/combo/numbers/markers), then
    // count it down. Otherwise merge the physical trigger into RT normally (guns shooting / held attack).
    float meleeImpulse = OpenXRManager::Get().GetSharedSlot(29);
    if (meleeImpulse > 0.5f) {
        pState->Gamepad.bRightTrigger = 255;
        OpenXRManager::Get().SetSharedSlot(29, meleeImpulse - 1.0f);
    } else {
        if (rt > pState->Gamepad.bRightTrigger) pState->Gamepad.bRightTrigger = rt;
    }

    // Left stick = locomotion (always merged when magnitude exceeds the
    // physical pad's so the game uses our values).
    float lx = ApplyStickDeadzone(vr.leftThumbX, 0.12f);
    float ly = ApplyStickDeadzone(vr.leftThumbY, 0.12f);
    if (fabsf(lx) > fabsf(pState->Gamepad.sThumbLX / 32767.0f)) pState->Gamepad.sThumbLX = FloatToSHORT(lx);
    if (fabsf(ly) > fabsf(pState->Gamepad.sThumbLY / 32767.0f)) pState->Gamepad.sThumbLY = FloatToSHORT(ly);

    // Left stick pushed near FULL forward => SPRINT. A partial push is left as the
    // game's normal jog; only "to the stop" sprints. CP2077 sprint is the left-stick
    // click (L3), so we just assert L3 while the stick is forward past the threshold
    // -- no more clicking the stick. Level-triggered (held while past the threshold)
    // mirrors physically holding L3: correct for hold-to-sprint, and toggle-sprint
    // auto-cancels on slow-down so it stays in sync as well.
    const bool wantSprint = (ly > 0.90f);

    // Right stick = camera turn / pitch.
    float rx = ApplyStickDeadzone(vr.rightThumbX, 0.18f);
    float ry = ApplyStickDeadzone(vr.rightThumbY, 0.18f);

    // Right stick pushed near FULL down => CROUCH. Same bind as the right-stick click
    // (R3) used today; we assert R3 while the stick is held fully down and consume the
    // downward Y so it doesn't also drive camera pitch. Detected here, before the snap
    // turn block may zero ry, so it works regardless of the turn mode.
    const bool wantCrouch = (ry < -0.90f);
    if (wantCrouch) ry = 0.0f;

    // Suppress pitch from the stick if the user wants HMD-only pitch.
    if (g_liveControls.xrDisableMouseY != 0) ry = 0.0f;

    if (g_liveControls.xrSnapTurn != 0) {
        // True instant snap turn: route the right-stick flick directly into a
        // yaw delta the game applies in ONE frame via the OnFootDeltaHead hook.
        // Stick X is consumed (zeroed) so the game never sees stick-driven
        // smooth rotation. Stick must recenter (|rx|<0.15) before another snap
        // can fire -- held stick produces exactly one snap.
        int wantDir = 0;
        if (rx > 0.5f) wantDir = +1;
        else if (rx < -0.5f) wantDir = -1;

        if (fabsf(rx) < 0.15f) g_xinputSnapArmedDir = 0;

        if (wantDir != 0 && wantDir != g_xinputSnapArmedDir) {
            g_xinputSnapArmedDir = wantDir;
            const float angleDeg = g_liveControls.xrSnapTurnAngleDeg > 0.0f
                ? g_liveControls.xrSnapTurnAngleDeg : 30.0f;
            // In CP2077 the on-foot yaw delta is signed such that positive =
            // turn LEFT, so we negate wantDir to make stick-right -> turn right.
            const float deltaDeg = -(float)wantDir * angleDeg;
            LONG bits;
            memcpy(&bits, &deltaDeg, sizeof(bits));
            InterlockedExchange(&g_pendingSnapYawDeltaBits, bits);
        }
        // Stick X is consumed by the snap turn, so do not pass it to the game.
        rx = 0.0f;
        ry = 0.0f;
    }

    if (fabsf(rx) > fabsf(pState->Gamepad.sThumbRX / 32767.0f)) pState->Gamepad.sThumbRX = FloatToSHORT(rx);
    if (fabsf(ry) > fabsf(pState->Gamepad.sThumbRY / 32767.0f)) pState->Gamepad.sThumbRY = FloatToSHORT(ry);

    // Stick-gesture buttons: full-forward left stick => sprint (L3), full-down right
    // stick => crouch (R3). OR'd in on top of any physical / VR button press.
    uint16_t synthButtons = 0;
    if (wantSprint) synthButtons |= XINPUT_GAMEPAD_LEFT_THUMB;
    if (wantCrouch) synthButtons |= XINPUT_GAMEPAD_RIGHT_THUMB;
    pState->Gamepad.wButtons |= synthButtons;

    // Bump packet number on any change so XInput consumers latch it.
    static uint16_t s_lastButtons = 0;
    static uint16_t s_lastSynth = 0;
    static BYTE s_lastLT = 0, s_lastRT = 0;
    if (vr.buttons != s_lastButtons || synthButtons != s_lastSynth || lt != s_lastLT || rt != s_lastRT) {
        pState->dwPacketNumber++;
        s_lastButtons = vr.buttons;
        s_lastSynth = synthButtons;
        s_lastLT = lt;
        s_lastRT = rt;
    }
    return r;
}

bool InstallXInputHook() {
    HMODULE m = LoadLibraryA("XInput1_4.dll");
    if (!m) m = LoadLibraryA("XInput1_3.dll");
    if (!m) m = LoadLibraryA("xinput9_1_0.dll");
    if (!m) {
        Log("XInput: no XInput DLL found\n");
        return false;
    }
    void* fn = GetProcAddress(m, "XInputGetState");
    if (!fn) {
        // Some builds export ordinal-only "XInputGetStateEx" (#100). For our needs
        // XInputGetState is fine; skip if absent.
        Log("XInput: GetProcAddress(XInputGetState) failed\n");
        return false;
    }

    // 14-byte detour stub: load 64-bit absolute into rax, jmp rax. Save the
    // original 14 bytes so we can call the real function from a trampoline.
    g_xinputHookAt = static_cast<uint8_t*>(fn);
    memcpy(g_xinputOrig, g_xinputHookAt, sizeof(g_xinputOrig));

    // Trampoline = original 14 bytes + jmp back to (hookAt + 14).
    void* tramp = AllocateTrampoline(g_xinputHookAt, 64);
    if (!tramp) {
        Log("XInput: AllocateTrampoline failed\n");
        return false;
    }
    uint8_t* tcode = static_cast<uint8_t*>(tramp);
    int tpos = 0;
    memcpy(tcode + tpos, g_xinputOrig, 14); tpos += 14;
    tcode[tpos++] = 0xFF; tcode[tpos++] = 0x25; // jmp [rip+0]
    *reinterpret_cast<int32_t*>(tcode + tpos) = 0; tpos += 4;
    *reinterpret_cast<uint64_t*>(tcode + tpos) = reinterpret_cast<uint64_t>(g_xinputHookAt + 14);
    tpos += 8;
    g_realXInputGetState = reinterpret_cast<XInputGetState_t>(tcode);

    // Patch the real entry point with: movabs rax, &Hooked; jmp rax (12 bytes).
    DWORD oldProtect = 0;
    if (!VirtualProtect(g_xinputHookAt, 14, PAGE_EXECUTE_READWRITE, &oldProtect)) {
        Log("XInput: VirtualProtect failed\n");
        return false;
    }
    uint8_t* p = g_xinputHookAt;
    p[0] = 0x48; p[1] = 0xB8; // mov rax, imm64
    *reinterpret_cast<uint64_t*>(p + 2) = reinterpret_cast<uint64_t>(&HookedXInputGetState);
    p[10] = 0xFF; p[11] = 0xE0; // jmp rax
    p[12] = 0x90; p[13] = 0x90;
    VirtualProtect(g_xinputHookAt, 14, oldProtect, &oldProtect);
    FlushInstructionCache(GetCurrentProcess(), g_xinputHookAt, 14);
    g_xinputHooked = true;
    Log("XInput: hook installed at %p (trampoline=%p)\n", g_xinputHookAt, tcode);
    return true;
}

DWORD WINAPI WorkerThread(LPVOID) {
    if (g_verboseLog) Log("Worker thread started, waiting 8 seconds...\n");
    if (g_backendModulePath[0] != '\0') {
        if (g_verboseLog) Log("Backend module loaded from: %s\n", g_backendModulePath);
    }
    Sleep(8000);

    EnsureLiveControlFileExists();
    PollLiveControls();
    InitGameModuleInfo();

    // Allocate telemetry structure
    g_telemetry = static_cast<TelemetryData*>(VirtualAlloc(nullptr, sizeof(TelemetryData), MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE));
    ZeroMemory(g_telemetry, sizeof(TelemetryData));

    g_setterTrace = static_cast<SetterTraceData*>(VirtualAlloc(nullptr, sizeof(SetterTraceData), MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE));
    ZeroMemory(g_setterTrace, sizeof(SetterTraceData));

    bool h1 = InstallLocateCameraHook();
    if (g_verboseLog || !h1) Log("LocateCamera hook result: %s\n", h1 ? "SUCCESS" : "FAILED");

    bool h2 = InstallPatchCameraHook();
    if (g_verboseLog || !h2) Log("PatchCamera hook result: %s\n", h2 ? "SUCCESS" : "FAILED");

    bool h3 = InstallFinalCameraHook();
    if (g_verboseLog || !h3) Log("FinalCamera hook result: %s\n", h3 ? "SUCCESS" : "FAILED");

    bool h_pitch = InstallPitchHook();
    g_pitchHookInstalled = h_pitch;
    if (g_verboseLog || !h_pitch) Log("Pitch hook result: %s\n", h_pitch ? "SUCCESS" : "FAILED");

    bool h_fov = InstallNormalFovHook();
    g_normalFovHookInstalled = h_fov;
    if (g_verboseLog || !h_fov) Log("NormalFOV hook result: %s\n", h_fov ? "SUCCESS" : "FAILED");

    bool h_lod = InstallFixLoDHook();
    if (g_verboseLog || !h_lod) Log("FixLoD hook result: %s\n", h_lod ? "SUCCESS" : "FAILED");

    bool h_menu = InstallMenuModeHook();
    if (g_verboseLog || !h_menu) Log("MenuMode hook result: %s\n", h_menu ? "SUCCESS" : "FAILED");

    bool h_heading_force = InstallForceHeadingUpdateHook();
    g_forceHeadingUpdateHookInstalled = h_heading_force;
    if (g_verboseLog || !h_heading_force) Log("ForceHeadingUpdate hook result: %s\n", h_heading_force ? "SUCCESS" : "FAILED");

    bool h4 = InstallOnFootDeltaHeadHook();
    if (g_verboseLog || !h4) Log("OnFootDeltaHead hook result: %s\n", h4 ? "SUCCESS" : "FAILED");

    bool h5 = InstallOnFootMoveXYHook();
    if (g_verboseLog || !h5) Log("OnFootMoveXY hook result: %s\n", h5 ? "SUCCESS" : "FAILED");

    if (g_liveControls.xrXInputInstall != 0) {
        bool h_xinput = InstallXInputHook();
        if (g_verboseLog || !h_xinput) Log("XInput hook result: %s\n", h_xinput ? "SUCCESS" : "FAILED");
    }

    bool h6 = InstallFreeDeltaHeadHook();
    if (g_verboseLog || !h6) Log("FreeDeltaHead hook result: %s\n", h6 ? "SUCCESS" : "FAILED");

    if (kEnablePatchBufferTracer != 0) {
        bool h_pb = InstallPatchBufferHook();
        if (g_verboseLog || !h_pb) Log("PatchBuffer hook result: %s\n", h_pb ? "SUCCESS" : "FAILED");
    }

    bool h10 = InstallSettingsResHook();
    if (g_verboseLog || !h10) Log("SettingsRes hook result: %s\n", h10 ? "SUCCESS" : "FAILED");

    bool h11 = InstallDLSSResHook();
    if (g_verboseLog || !h11) Log("DLSSRes hook result: %s\n", h11 ? "SUCCESS" : "FAILED");

    bool h12 = InstallDLSSMatricesHook();
    if (g_verboseLog || !h12) Log("DLSSMatrices hook result: %s\n", h12 ? "SUCCESS" : "FAILED");

    if (kEnableNativeSetterTracers != 0) {
        bool h7 = InstallNativeSetterMetaWriteHook();
        if (g_verboseLog || !h7) Log("NativeSetterMetaWrite hook result: %s\n", h7 ? "SUCCESS" : "FAILED");

        bool h8 = InstallNativeSetterMetaConsumeHook();
        if (g_verboseLog || !h8) Log("NativeSetterMetaConsume hook result: %s\n", h8 ? "SUCCESS" : "FAILED");

        bool h9 = InstallNativeSetterClearHook();
        if (g_verboseLog || !h9) Log("NativeSetterClear hook result: %s\n", h9 ? "SUCCESS" : "FAILED");
    }

    uint32_t prevLocateHits = 0;
    uint32_t prevPatchHits = 0;
    uint32_t prevFinalHits = 0;
    uint32_t prevDeltaHeadHits = 0;
    uint32_t prevMoveXYHits = 0;
    uint32_t prevFreeDeltaHits = 0;
    uintptr_t prevPatchRdx = 0;
    uintptr_t prevPatchRsi = 0;
    uintptr_t prevFinalRsi = 0;
    uintptr_t prevDeltaHeadRcx = 0;
    uintptr_t prevMoveXYRsi = 0;
    uintptr_t prevFreeDeltaRsi = 0;
    uint32_t prevMetaWriteHits = 0;
    uint32_t prevMetaConsumeHits = 0;
    uint32_t prevClearHits = 0;
    uintptr_t prevMetaWriteTemp = 0;
    uintptr_t prevMetaWriteMeta = 0;
    uintptr_t prevMetaConsumeTemp = 0;
    uintptr_t prevMetaConsumeMeta = 0;
    uintptr_t prevMetaWriteRsp = 0;
    uintptr_t prevMetaConsumeRsp = 0;
    uintptr_t prevClearTemp = 0;
    uintptr_t prevClearReturn = 0;
    uint32_t loopCounter = 0;

    for (;;) {
        PollLiveControls();
        PollHotkeys();
        ApplyKnownResolutionOverrides();

        if ((loopCounter++ % 10) != 0) {
            Sleep(200);
            continue;
        }

        // The periodic telemetry dump below is pure diagnostics; skip it entirely
        // (and its bookkeeping) unless verbose logging is on. PollLiveControls /
        // PollHotkeys / resolution overrides above still run every iteration.
        if (!g_verboseLog) {
            Sleep(200);
            continue;
        }

        uint32_t lHits = g_telemetry->locateHits;
        uint32_t pHits = g_telemetry->patchHits;
        uint32_t fHits = g_telemetry->finalHits;
        
        Log("--- TELEMETRY SAMPLE ---\n");
        Log("LocateCamera: hits=%u, rbx=%p, xmm0(f32)=%.6f\n", 
            lHits, reinterpret_cast<void*>(g_telemetry->locateRbx), g_telemetry->locateXmm0);
        
        Log("PatchCamera:  hits=%u, rdx=%p, xmm0=(%.6f, %.6f, %.6f, %.6f)\n", 
            pHits, reinterpret_cast<void*>(g_telemetry->patchRdx), 
            g_telemetry->patchXmm0[0], g_telemetry->patchXmm0[1], 
            g_telemetry->patchXmm0[2], g_telemetry->patchXmm0[3]);
        Log("PatchCamera:  rsi=%p\n", reinterpret_cast<void*>(g_telemetry->patchRsi));

        Log("FinalCamera:  hits=%u, rsi=%p\n", 
            fHits, reinterpret_cast<void*>(g_telemetry->finalRsi));

        Log("DeltaHead:    hits=%u, rcx=%p, xmm0=%.6f\n",
            g_telemetry->deltaHeadHits,
            reinterpret_cast<void*>(g_telemetry->deltaHeadRcx),
            g_telemetry->deltaHeadXmm0);

        Log("MoveXY:       hits=%u, rsi=%p, xmm0=%.6f\n",
            g_telemetry->moveXYHits,
            reinterpret_cast<void*>(g_telemetry->moveXYRsi),
            g_telemetry->moveXYXmm0);

        Log("FreeDelta:    hits=%u, rsi=%p, xmm3=%.6f\n",
            g_telemetry->freeDeltaHits,
            reinterpret_cast<void*>(g_telemetry->freeDeltaRsi),
            g_telemetry->freeDeltaXmm3);

        Log("DLSSMatrices: hits=%llu, this=%p, state=%p, slot=%u, adjusted=%u, eye=%d, enabled=%d, mode=%d\n",
            static_cast<unsigned long long>(g_dlssMatricesHits),
            reinterpret_cast<void*>(g_dlssMatricesThis),
            reinterpret_cast<void*>(g_dlssMatricesState),
            g_dlssMatricesSlot,
            g_dlssMatricesAdjustedSlot,
            g_dlssMatricesEye,
            g_liveControls.xrDLSSMatrixHook,
            g_liveControls.xrDLSSSlotMode);

        if (g_telemetry->patchRdx != prevPatchRdx || g_telemetry->patchRsi != prevPatchRsi) {
            uintptr_t patchRdx = g_telemetry->patchRdx;
            uintptr_t patchRsi = g_telemetry->patchRsi;
            LogVec4At("patch rdx-0x10", patchRdx ? patchRdx - 0x10 : 0);
            LogVec4At("patch rdx+0x00", patchRdx);
            LogVec4At("patch rdx+0x10", patchRdx ? patchRdx + 0x10 : 0);
            LogVec4At("patch rdx+0x20", patchRdx ? patchRdx + 0x20 : 0);
            LogU8At("patch rsi+0xB0", patchRsi ? patchRsi + 0xB0 : 0);
            LogU8At("patch rsi+0xB1", patchRsi ? patchRsi + 0xB1 : 0);
            LogVec4At("patch rsi+0x90", patchRsi ? patchRsi + 0x90 : 0);
            LogVec4At("patch rsi+0xA0", patchRsi ? patchRsi + 0xA0 : 0);
        }

        if (g_telemetry->finalRsi != prevFinalRsi) {
            uintptr_t finalRsi = g_telemetry->finalRsi;
            LogFloatAt("final rsi+0x40", finalRsi ? finalRsi + 0x40 : 0);
            LogFloatAt("final rsi+0x44", finalRsi ? finalRsi + 0x44 : 0);
            LogVec4At("final rsi+0x30", finalRsi ? finalRsi + 0x30 : 0);
            LogVec4At("final rsi+0x40", finalRsi ? finalRsi + 0x40 : 0);
        }

        if (g_telemetry->deltaHeadRcx != prevDeltaHeadRcx || g_telemetry->deltaHeadHits != prevDeltaHeadHits) {
            uintptr_t deltaHeadRcx = g_telemetry->deltaHeadRcx;
            LogFloatAt("delta rcx+0x98", deltaHeadRcx ? deltaHeadRcx + 0x98 : 0);
            LogFloatAt("delta rcx+0x9C", deltaHeadRcx ? deltaHeadRcx + 0x9C : 0);
            LogFloatAt("delta rcx+0xA0", deltaHeadRcx ? deltaHeadRcx + 0xA0 : 0);
            LogFloatAt("delta rcx+0xA4", deltaHeadRcx ? deltaHeadRcx + 0xA4 : 0);
            LogFloatAt("delta rcx+0xA8", deltaHeadRcx ? deltaHeadRcx + 0xA8 : 0);
        }

        if (g_telemetry->moveXYRsi != prevMoveXYRsi || g_telemetry->moveXYHits != prevMoveXYHits) {
            uintptr_t moveXYRsi = g_telemetry->moveXYRsi;
            LogFloatAt("moveXY rsi+0x90", moveXYRsi ? moveXYRsi + 0x90 : 0);
            LogFloatAt("moveXY rsi+0x94", moveXYRsi ? moveXYRsi + 0x94 : 0);
            LogFloatAt("moveXY rsi+0x98", moveXYRsi ? moveXYRsi + 0x98 : 0);
            LogFloatAt("moveXY rsi+0x9C", moveXYRsi ? moveXYRsi + 0x9C : 0);
        }

        if (g_telemetry->freeDeltaRsi != prevFreeDeltaRsi || g_telemetry->freeDeltaHits != prevFreeDeltaHits) {
            uintptr_t freeDeltaRsi = g_telemetry->freeDeltaRsi;
            LogFloatAt("freeDelta rsi+0xCC8", freeDeltaRsi ? freeDeltaRsi + 0xCC8 : 0);
            LogFloatAt("freeDelta rsi+0xCCC", freeDeltaRsi ? freeDeltaRsi + 0xCCC : 0);
            LogFloatAt("freeDelta rsi+0x208", freeDeltaRsi ? freeDeltaRsi + 0x208 : 0);
            LogFloatAt("freeDelta rsi+0x20C", freeDeltaRsi ? freeDeltaRsi + 0x20C : 0);
        }

        if ((g_setterTrace->metaWriteHits != 0 && prevMetaWriteHits == 0) ||
            g_setterTrace->metaWriteTemp != prevMetaWriteTemp ||
            g_setterTrace->metaWriteMeta != prevMetaWriteMeta ||
            g_setterTrace->metaWriteRsp != prevMetaWriteRsp) {
            Log("NativeSetMetaWrite: hits=%u, temp=%p, meta=%p, rsp=%p\n",
                g_setterTrace->metaWriteHits,
                reinterpret_cast<void*>(g_setterTrace->metaWriteTemp),
                reinterpret_cast<void*>(g_setterTrace->metaWriteMeta),
                reinterpret_cast<void*>(g_setterTrace->metaWriteRsp));
            LogVec4At("metaWrite temp+0x00", g_setterTrace->metaWriteTemp);
            LogPtrAt("metaWrite temp+0x08", g_setterTrace->metaWriteTemp ? g_setterTrace->metaWriteTemp + 0x08 : 0);
            LogPtrAt("metaWrite meta+0x00", g_setterTrace->metaWriteMeta);
            LogPtrAt("metaWrite meta+0x08", g_setterTrace->metaWriteMeta ? g_setterTrace->metaWriteMeta + 0x08 : 0);
            LogPtrPayloadVec4At("metaWrite meta+0x10", g_setterTrace->metaWriteMeta ? g_setterTrace->metaWriteMeta + 0x10 : 0);
            LogStackWindowAt("metaWrite stack", g_setterTrace->metaWriteRsp, 12);
        }

        if ((g_setterTrace->metaConsumeHits != 0 && prevMetaConsumeHits == 0) ||
            g_setterTrace->metaConsumeTemp != prevMetaConsumeTemp ||
            g_setterTrace->metaConsumeMeta != prevMetaConsumeMeta ||
            g_setterTrace->metaConsumeRsp != prevMetaConsumeRsp) {
            Log("NativeSetMetaConsume: hits=%u, temp=%p, meta=%p, rsp=%p\n",
                g_setterTrace->metaConsumeHits,
                reinterpret_cast<void*>(g_setterTrace->metaConsumeTemp),
                reinterpret_cast<void*>(g_setterTrace->metaConsumeMeta),
                reinterpret_cast<void*>(g_setterTrace->metaConsumeRsp));
            LogVec4At("metaConsume temp+0x00", g_setterTrace->metaConsumeTemp);
            LogPtrAt("metaConsume temp+0x08", g_setterTrace->metaConsumeTemp ? g_setterTrace->metaConsumeTemp + 0x08 : 0);
            LogPtrAt("metaConsume meta+0x00", g_setterTrace->metaConsumeMeta);
            LogPtrAt("metaConsume meta+0x08", g_setterTrace->metaConsumeMeta ? g_setterTrace->metaConsumeMeta + 0x08 : 0);
            LogPtrPayloadVec4At("metaConsume meta+0x10", g_setterTrace->metaConsumeMeta ? g_setterTrace->metaConsumeMeta + 0x10 : 0);
            LogStackWindowAt("metaConsume stack", g_setterTrace->metaConsumeRsp, 12);
        }

        if ((g_setterTrace->clearHits != 0 && prevClearHits == 0) ||
            g_setterTrace->clearTemp != prevClearTemp ||
            g_setterTrace->clearReturn != prevClearReturn) {
            Log("NativeSetClear: hits=%u, temp=%p, return=%p\n",
                g_setterTrace->clearHits,
                reinterpret_cast<void*>(g_setterTrace->clearTemp),
                reinterpret_cast<void*>(g_setterTrace->clearReturn));
            LogVec4At("clear temp+0x00", g_setterTrace->clearTemp);
            LogPtrAt("clear temp+0x08", g_setterTrace->clearTemp ? g_setterTrace->clearTemp + 0x08 : 0);
        }

        prevLocateHits = lHits;
        prevPatchHits = pHits;
        prevFinalHits = fHits;
        prevDeltaHeadHits = g_telemetry->deltaHeadHits;
        prevMoveXYHits = g_telemetry->moveXYHits;
        prevFreeDeltaHits = g_telemetry->freeDeltaHits;
        prevPatchRdx = g_telemetry->patchRdx;
        prevPatchRsi = g_telemetry->patchRsi;
        prevFinalRsi = g_telemetry->finalRsi;
        prevDeltaHeadRcx = g_telemetry->deltaHeadRcx;
        prevMoveXYRsi = g_telemetry->moveXYRsi;
        prevFreeDeltaRsi = g_telemetry->freeDeltaRsi;
        prevMetaWriteHits = g_setterTrace->metaWriteHits;
        prevMetaConsumeHits = g_setterTrace->metaConsumeHits;
        prevClearHits = g_setterTrace->clearHits;
        prevMetaWriteTemp = g_setterTrace->metaWriteTemp;
        prevMetaWriteMeta = g_setterTrace->metaWriteMeta;
        prevMetaConsumeTemp = g_setterTrace->metaConsumeTemp;
        prevMetaConsumeMeta = g_setterTrace->metaConsumeMeta;
        prevMetaWriteRsp = g_setterTrace->metaWriteRsp;
        prevMetaConsumeRsp = g_setterTrace->metaConsumeRsp;
        prevClearTemp = g_setterTrace->clearTemp;
        prevClearReturn = g_setterTrace->clearReturn;

        Sleep(200);
    }
    return 0;
}

BOOL WINAPI DllMain(HINSTANCE hinst, DWORD reason, LPVOID) {
    if (reason == DLL_PROCESS_ATTACH) {
        DisableThreadLibraryCalls(hinst);
        InitRuntimePaths();

        // Always use system dxgi.dll for exports
        char path[MAX_PATH];
        GetSystemDirectoryA(path, MAX_PATH);
        strcat_s(path, "\\dxgi.dll");
        g_realDxgi = LoadLibraryA(path);

        HANDLE h = CreateThread(nullptr, 0, WorkerThread, nullptr, 0, nullptr);
        if (h) CloseHandle(h);
    } else if (reason == DLL_PROCESS_DETACH) {
        OpenXRManager::Get().Shutdown();
    }
    return TRUE;
}

extern "C" {
    FARPROC GetRealProc(const char* name) {
        if (!g_realDxgi) return nullptr;
        return GetProcAddress(g_realDxgi, name);
    }

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

using PFN_CreateDXGIFactory_Proxy = HRESULT(WINAPI*)(REFIID, void**);
using PFN_CreateDXGIFactory2_Proxy = HRESULT(WINAPI*)(UINT, REFIID, void**);
using PFN_DXGIDeclareAdapterRemovalSupport_Proxy = HRESULT(WINAPI*)();
using PFN_DXGIDisableVBlankVirtualization_Proxy = HRESULT(WINAPI*)();
using PFN_DXGIGetDebugInterface1_Proxy = HRESULT(WINAPI*)(UINT, REFIID, void**);
using PFN_DXGIReportAdapterConfiguration_Proxy = HRESULT(WINAPI*)(DWORD);

// Enable DRED auto-breadcrumbs + page-fault reporting before any D3D12 device
// is created. Implemented in dxgi_factory_wrapper.cpp.
extern "C" void CyberpunkVRPort_EnableDredOnce();

extern "C" __declspec(dllexport) HRESULT WINAPI CreateDXGIFactory(REFIID riid, void** ppFactory) {
    CyberpunkVRPort_EnableDredOnce();
    PrepareStartupLiveControls();
    InitOpenXREarly();
    auto p = reinterpret_cast<PFN_CreateDXGIFactory_Proxy>(GetRealProc("CreateDXGIFactory"));
    if (!p) return E_FAIL;
    IDXGIFactory7* realFact = nullptr;
    HRESULT hr = p(__uuidof(IDXGIFactory7), reinterpret_cast<void**>(&realFact));
    if (FAILED(hr) || !realFact) return hr;

    DXGIFactoryWrapper* wrapper = new DXGIFactoryWrapper(realFact);
    hr = wrapper->QueryInterface(riid, ppFactory);
    wrapper->Release();
    return hr;
}

extern "C" __declspec(dllexport) HRESULT WINAPI CreateDXGIFactory1(REFIID riid, void** ppFactory) {
    CyberpunkVRPort_EnableDredOnce();
    PrepareStartupLiveControls();
    InitOpenXREarly();
    auto p = reinterpret_cast<PFN_CreateDXGIFactory_Proxy>(GetRealProc("CreateDXGIFactory1"));
    if (!p) return E_FAIL;
    IDXGIFactory7* realFact = nullptr;
    HRESULT hr = p(__uuidof(IDXGIFactory7), reinterpret_cast<void**>(&realFact));
    if (FAILED(hr) || !realFact) return hr;

    DXGIFactoryWrapper* wrapper = new DXGIFactoryWrapper(realFact);
    hr = wrapper->QueryInterface(riid, ppFactory);
    wrapper->Release();
    return hr;
}

extern "C" __declspec(dllexport) HRESULT WINAPI CreateDXGIFactory2(UINT Flags, REFIID riid, void** ppFactory) {
    CyberpunkVRPort_EnableDredOnce();
    PrepareStartupLiveControls();
    InitOpenXREarly();
    auto p = reinterpret_cast<PFN_CreateDXGIFactory2_Proxy>(GetRealProc("CreateDXGIFactory2"));
    if (!p) return E_FAIL;
    IDXGIFactory7* realFact = nullptr;
    HRESULT hr = p(Flags, __uuidof(IDXGIFactory7), reinterpret_cast<void**>(&realFact));
    if (FAILED(hr) || !realFact) return hr;

    DXGIFactoryWrapper* wrapper = new DXGIFactoryWrapper(realFact);
    hr = wrapper->QueryInterface(riid, ppFactory);
    wrapper->Release();
    return hr;
}

    __declspec(dllexport) HRESULT WINAPI DXGIDeclareAdapterRemovalSupport() {
        auto p = reinterpret_cast<PFN_DXGIDeclareAdapterRemovalSupport_Proxy>(GetRealProc("DXGIDeclareAdapterRemovalSupport"));
        return p ? p() : E_FAIL;
    }

    __declspec(dllexport) HRESULT WINAPI DXGIDisableVBlankVirtualization() {
        auto p = reinterpret_cast<PFN_DXGIDisableVBlankVirtualization_Proxy>(GetRealProc("DXGIDisableVBlankVirtualization"));
        return p ? p() : E_FAIL;
    }

    __declspec(dllexport) HRESULT WINAPI DXGIGetDebugInterface1(UINT Flags, REFIID riid, void** pDebug) {
        auto p = reinterpret_cast<PFN_DXGIGetDebugInterface1_Proxy>(GetRealProc("DXGIGetDebugInterface1"));
        return p ? p(Flags, riid, pDebug) : E_FAIL;
    }

    __declspec(dllexport) HRESULT WINAPI DXGIReportAdapterConfiguration(DWORD arg) {
        auto p = reinterpret_cast<PFN_DXGIReportAdapterConfiguration_Proxy>(GetRealProc("DXGIReportAdapterConfiguration"));
        return p ? p(arg) : E_FAIL;
    }
}
