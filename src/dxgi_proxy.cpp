#include <windows.h>
#include <psapi.h>
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
static char g_backendModulePath[MAX_PATH] = {};
static FILETIME g_lastLiveControlWrite = {};
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
    volatile int xrWindowWidth;
    volatile int xrWindowHeight;
    volatile float xrForceFov;
    volatile int xrMenuRect;
    volatile float xrMenuFov;
    volatile float xrPitchSign;
    volatile float xrPitchScale;
    volatile int xrSyncSequential;
    volatile int xr3DofMovement;
    volatile int xrDLSSMatrixHook;
    volatile int xrDLSSSlotMode;
    volatile int xrDLSSLogStride;
    volatile int xrAERPairGate;
    volatile int xrAERStartEye;
    volatile int xrAERDebugEye;
    volatile int xrAERWarmupFrames;
};

static LiveControls g_liveControls = {};

static void InitRuntimePaths() {
    if (g_gameDir[0] != '\0') return;

    GetModuleFileNameA(nullptr, g_gameDir, MAX_PATH);
    char* lastSlash = strrchr(g_gameDir, '\\');
    if (lastSlash) {
        *lastSlash = '\0';
    }

    strcpy_s(g_liveControlPath, g_gameDir);
    strcat_s(g_liveControlPath, "\\vrport.ini");
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
    fprintf(file, "xr_window_width=0\n");
    fprintf(file, "xr_window_height=0\n");
    fprintf(file, "xr_force_fov=0\n");
    fprintf(file, "xr_menu_rect=0\n");
    fprintf(file, "xr_menu_fov=65.0\n");
    fprintf(file, "xr_pitch_sign=1.0\n");
    fprintf(file, "xr_pitch_scale=1.35\n");
    fprintf(file, "xr_sync_sequential=1\n");
    fprintf(file, "xr_3dof_movement=1\n");
    fprintf(file, "xr_dlss_matrix_hook=1\n");
    fprintf(file, "xr_dlss_slot_mode=0\n");
    fprintf(file, "xr_dlss_log_stride=600\n");
    fprintf(file, "xr_aer_pair_gate=1\n");
    fprintf(file, "xr_aer_start_eye=0\n");
    fprintf(file, "xr_aer_debug_eye=0\n");
    fprintf(file, "xr_aer_warmup_frames=1\n");
    fclose(file);
}

static void PollLiveControls() {
    InitRuntimePaths();

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
        g_liveControls.xrWindowWidth != xrWindowWidth ||
        g_liveControls.xrWindowHeight != xrWindowHeight ||
        g_liveControls.xrForceFov != xrForceFov ||
        g_liveControls.xrMenuRect != xrMenuRect ||
        g_liveControls.xrMenuFov != xrMenuFov ||
        g_liveControls.xrPitchSign != xrPitchSign ||
        g_liveControls.xrPitchScale != xrPitchScale ||
        g_liveControls.xrSyncSequential != xrSyncSequential ||
        g_liveControls.xr3DofMovement != xr3DofMovement ||
        g_liveControls.xrDLSSMatrixHook != xrDLSSMatrixHook ||
        g_liveControls.xrDLSSSlotMode != xrDLSSSlotMode ||
        g_liveControls.xrDLSSLogStride != xrDLSSLogStride ||
        g_liveControls.xrAERPairGate != xrAERPairGate ||
        g_liveControls.xrAERStartEye != xrAERStartEye ||
        g_liveControls.xrAERDebugEye != xrAERDebugEye ||
        g_liveControls.xrAERWarmupFrames != xrAERWarmupFrames;

    g_liveControls.xrHeadOffsetX = xrHeadOffsetX;
    g_liveControls.xrHeadOffsetY = xrHeadOffsetY;
    g_liveControls.xrHeadOffsetZ = xrHeadOffsetZ;
    g_liveControls.xrRecenter = xrRecenter;
    g_liveControls.xrMonoSubmit = xrMonoSubmit;
    g_liveControls.xrAERSubmit = xrAERSubmit;
    g_liveControls.xrWindowWidth = xrWindowWidth;
    g_liveControls.xrWindowHeight = xrWindowHeight;
    g_liveControls.xrForceFov = xrForceFov;
    g_liveControls.xrMenuRect = xrMenuRect;
    g_liveControls.xrMenuFov = xrMenuFov;
    g_liveControls.xrPitchSign = xrPitchSign;
    g_liveControls.xrPitchScale = xrPitchScale;
    g_liveControls.xrSyncSequential = xrSyncSequential;
    g_liveControls.xr3DofMovement = xr3DofMovement;
    g_liveControls.xrDLSSMatrixHook = xrDLSSMatrixHook;
    g_liveControls.xrDLSSSlotMode = xrDLSSSlotMode;
    g_liveControls.xrDLSSLogStride = xrDLSSLogStride > 0 ? xrDLSSLogStride : 0;
    g_liveControls.xrAERPairGate = xrAERPairGate;
    g_liveControls.xrAERStartEye = xrAERStartEye != 0 ? 1 : 0;
    g_liveControls.xrAERDebugEye = xrAERDebugEye;
    g_liveControls.xrAERWarmupFrames = xrAERWarmupFrames > 0 ? xrAERWarmupFrames : 0;
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

    if (changed) {
        Log("Live controls updated: xr_head_offset=(%.4f,%.4f,%.4f) xr_recenter=%d xr_mono_submit=%d xr_aer_submit=%d xr_force_fov=%.3f xr_menu_rect=%d xr_menu_fov=%.3f xr_pitch_sign=%.1f xr_pitch_scale=%.3f xr_sync_sequential=%d xr_3dof_movement=%d xr_dlss_matrix_hook=%d xr_dlss_slot_mode=%d xr_dlss_log_stride=%d xr_aer_pair_gate=%d xr_aer_start_eye=%d xr_aer_debug_eye=%d xr_aer_warmup_frames=%d\n",
            g_liveControls.xrHeadOffsetX, g_liveControls.xrHeadOffsetY, g_liveControls.xrHeadOffsetZ, g_liveControls.xrRecenter, g_liveControls.xrMonoSubmit, g_liveControls.xrAERSubmit, g_liveControls.xrForceFov, g_liveControls.xrMenuRect, g_liveControls.xrMenuFov, g_liveControls.xrPitchSign, g_liveControls.xrPitchScale, g_liveControls.xrSyncSequential, g_liveControls.xr3DofMovement, g_liveControls.xrDLSSMatrixHook, g_liveControls.xrDLSSSlotMode, g_liveControls.xrDLSSLogStride, g_liveControls.xrAERPairGate, g_liveControls.xrAERStartEye, g_liveControls.xrAERDebugEye, g_liveControls.xrAERWarmupFrames);
        Log("Window override: xr_window_width=%d xr_window_height=%d\n",
            g_liveControls.xrWindowWidth,
            g_liveControls.xrWindowHeight);
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
    state.xrWindowWidth = g_liveControls.xrWindowWidth;
    state.xrWindowHeight = g_liveControls.xrWindowHeight;
    state.xrForceFov = g_liveControls.xrForceFov;
    state.xrMenuRect = g_liveControls.xrMenuRect;
    state.xrMenuFov = g_liveControls.xrMenuFov;
    state.xrPitchSign = g_liveControls.xrPitchSign;
    state.xrPitchScale = g_liveControls.xrPitchScale;
    state.xrSyncSequential = g_liveControls.xrSyncSequential;
    state.xr3DofMovement = g_liveControls.xr3DofMovement;
    state.xrDLSSMatrixHook = g_liveControls.xrDLSSMatrixHook;
    state.xrDLSSSlotMode = g_liveControls.xrDLSSSlotMode;
    state.xrDLSSLogStride = g_liveControls.xrDLSSLogStride;
    state.xrAERPairGate = g_liveControls.xrAERPairGate;
    state.xrAERStartEye = g_liveControls.xrAERStartEye;
    state.xrAERDebugEye = g_liveControls.xrAERDebugEye;
    state.xrAERWarmupFrames = g_liveControls.xrAERWarmupFrames;
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
    fprintf(file, "xr_window_width=%d\n", state.xrWindowWidth);
    fprintf(file, "xr_window_height=%d\n", state.xrWindowHeight);
    fprintf(file, "xr_force_fov=%.3f\n", state.xrForceFov);
    fprintf(file, "xr_menu_rect=%d\n", state.xrMenuRect != 0 ? 1 : 0);
    fprintf(file, "xr_menu_fov=%.3f\n", state.xrMenuFov);
    fprintf(file, "xr_pitch_sign=%.1f\n", state.xrPitchSign < 0.0f ? -1.0f : 1.0f);
    fprintf(file, "xr_pitch_scale=%.3f\n", state.xrPitchScale);
    fprintf(file, "xr_sync_sequential=%d\n", state.xrSyncSequential != 0 ? 1 : 0);
    fprintf(file, "xr_3dof_movement=%d\n", state.xr3DofMovement != 0 ? 1 : 0);
    fprintf(file, "xr_dlss_matrix_hook=%d\n", state.xrDLSSMatrixHook != 0 ? 1 : 0);
    fprintf(file, "xr_dlss_slot_mode=%d\n", state.xrDLSSSlotMode);
    fprintf(file, "xr_dlss_log_stride=%d\n", state.xrDLSSLogStride);
    fprintf(file, "xr_aer_pair_gate=%d\n", state.xrAERPairGate != 0 ? 1 : 0);
    fprintf(file, "xr_aer_start_eye=%d\n", state.xrAERStartEye != 0 ? 1 : 0);
    fprintf(file, "xr_aer_debug_eye=%d\n", state.xrAERDebugEye != 0 ? 1 : 0);
    fprintf(file, "xr_aer_warmup_frames=%d\n", state.xrAERWarmupFrames);
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
    g_liveControls.xrWindowWidth = state->xrWindowWidth > 0 ? state->xrWindowWidth : 0;
    g_liveControls.xrWindowHeight = state->xrWindowHeight > 0 ? state->xrWindowHeight : 0;
    g_liveControls.xrForceFov = state->xrForceFov > 0.0f ? state->xrForceFov : 0.0f;
    g_liveControls.xrMenuRect = state->xrMenuRect != 0 ? 1 : 0;
    g_liveControls.xrMenuFov = state->xrMenuFov > 1.0f ? state->xrMenuFov : 65.0f;
    g_liveControls.xrPitchSign = state->xrPitchSign < 0.0f ? -1.0f : 1.0f;
    g_liveControls.xrPitchScale = state->xrPitchScale > 0.01f ? state->xrPitchScale : 1.0f;
    g_liveControls.xrSyncSequential = state->xrSyncSequential != 0 ? 1 : 0;
    g_liveControls.xr3DofMovement = state->xr3DofMovement != 0 ? 1 : 0;
    g_liveControls.xrDLSSMatrixHook = state->xrDLSSMatrixHook != 0 ? 1 : 0;
    g_liveControls.xrDLSSSlotMode = state->xrDLSSSlotMode;
    g_liveControls.xrDLSSLogStride = state->xrDLSSLogStride > 0 ? state->xrDLSSLogStride : 0;
    g_liveControls.xrAERPairGate = state->xrAERPairGate != 0 ? 1 : 0;
    g_liveControls.xrAERStartEye = state->xrAERStartEye != 0 ? 1 : 0;
    g_liveControls.xrAERDebugEye = state->xrAERDebugEye != 0 ? 1 : 0;
    g_liveControls.xrAERWarmupFrames = state->xrAERWarmupFrames > 0 ? state->xrAERWarmupFrames : 0;

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

    if (!g_dialogShown) {
        g_dialogShown = true;
        ShowLauncherDialog();
    }
}

extern "C" void SetWindowResolutionAndPersist(int size) {
    g_liveControls.xrWindowWidth = size;
    g_liveControls.xrWindowHeight = size;
    PersistLiveControlsUiState(MakeLiveControlsUiState());
}

extern "C" int GetCurrentWindowWidth() {
    return g_liveControls.xrWindowWidth;
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
    if (g_liveControls.xrWindowWidth > 0) {
        return static_cast<UINT>(g_liveControls.xrWindowWidth);
    }
    return GetForcedRenderWidthValue();
}

static UINT GetForcedWindowHeightValue() {
    if (g_liveControls.xrWindowHeight > 0) {
        return static_cast<UINT>(g_liveControls.xrWindowHeight);
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
    return g_liveControls.xrWindowWidth > 0 ? static_cast<UINT>(g_liveControls.xrWindowWidth) : 0;
}

extern "C" UINT GetForcedSwapchainHeight() {
    return g_liveControls.xrWindowHeight > 0 ? static_cast<UINT>(g_liveControls.xrWindowHeight) : 0;
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
    return g_liveControls.xrSyncSequential;
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
    return g_liveControls.xrAERWarmupFrames;
}


void Log(const char* fmt, ...) {
    if (!g_logFile) {
        char logPath[MAX_PATH];
        GetModuleFileNameA(nullptr, logPath, MAX_PATH);
        char* lastSlash = strrchr(logPath, '\\');
        if (lastSlash) *(lastSlash + 1) = 0;
        strcat_s(logPath, "CyberpunkVRProxy.log");
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

static void ApplySettingsResolutionOverride(uintptr_t settingsPtr) {
    const UINT forcedWidth = GetForcedWindowWidthValue();
    const UINT forcedHeight = GetForcedWindowHeightValue();
    if (!settingsPtr || forcedWidth == 0 || forcedHeight == 0) {
        return;
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

    // VR Mod's CP2077DLSSRes hook feeds a single scalar through multiple size fields, so
    // keep the internal render target square here as well.
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
    const float runtimeFov = OpenXRManager::Get().GetRuntimeHorizontalFovDeg();
    return runtimeFov > 1.0f ? runtimeFov : 0.0f;
}

static float GetWorldScale() {
    return 1.0f;
}

static float GetDesiredHalfIpd() {
    const float runtimeIpd = OpenXRManager::Get().GetRuntimeIpd();
    const float halfIpd = runtimeIpd > 0.001f ? runtimeIpd * 0.5f : 0.032f;
    return halfIpd > 0.0001f ? halfIpd * GetWorldScale() : 0.0f;
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
static volatile float g_lastLocateQuat[4] = { 0.0f, 0.0f, 0.0f, 1.0f };
static volatile uint32_t g_lastLocateSeq = 0;

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

    OpenXRHeadPose xrPose{};
    const bool hasXR = OpenXRManager::Get().GetHeadPose(&xrPose);
    if (hasXR) {
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
        quat[0] = qx;
        quat[1] = qy;
        quat[2] = qz;
        quat[3] = qw;
    }

    if (hasXR) {
        const bool allowGameCameraTranslation = g_liveControls.xr3DofMovement == 0;
        const float posScale = allowGameCameraTranslation ? 1.0f * GetWorldScale() : 0.0f;

        // Map OpenXR local position into game-local camera space first:
        // XR: X=right, Y=up, -Z=forward
        // Game local here: X=right, Y=forward, Z=up
        const float localRight = xrPose.posX * posScale +
            (allowGameCameraTranslation ? g_liveControls.xrHeadOffsetX : 0.0f);
        const float localForward = -xrPose.posZ * posScale +
            (allowGameCameraTranslation ? g_liveControls.xrHeadOffsetY : 0.0f);
        const float localUp = xrPose.posY * posScale +
            (allowGameCameraTranslation ? g_liveControls.xrHeadOffsetZ : 0.0f);

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

        if ((g_locateCameraHits % 600) == 1) {
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

    g_lastLocatePosFP[0] = posFP[0];
    g_lastLocatePosFP[1] = posFP[1];
    g_lastLocatePosFP[2] = posFP[2];
    g_lastLocateQuat[0] = quat[0];
    g_lastLocateQuat[1] = quat[1];
    g_lastLocateQuat[2] = quat[2];
    g_lastLocateQuat[3] = quat[3];
    ++g_lastLocateSeq;
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

        Log("PatchCamera state @%p owner=%p flagB1=%u Q=(%.3f, %.3f, %.3f, %.3f) P0=(%.3f, %.3f, %.3f) P1=(%.3f, %.3f, %.3f) R=(%.3f, %.3f, %.3f) shift=%.4f applied=%d\n",
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

    if (!OpenXRManager::Get().IsAERSubmitEnabled()) return;
    if (!rsiPtr || reinterpret_cast<uintptr_t>(rsiPtr) < 0x10000) return;

    const uint32_t locateSeq = g_lastLocateSeq;
    float locateQuat[4] = {
        g_lastLocateQuat[0],
        g_lastLocateQuat[1],
        g_lastLocateQuat[2],
        g_lastLocateQuat[3]
    };
    if (locateSeq != 0) {
        int32_t* finalPosFP = reinterpret_cast<int32_t*>(rsiPtr);
        finalPosFP[0] = g_lastLocatePosFP[0];
        finalPosFP[1] = g_lastLocatePosFP[1];
        finalPosFP[2] = g_lastLocatePosFP[2];

        if (IsPlausibleUnitQuaternion(locateQuat)) {
            ApplyFinalCameraOrientationFromQuat(rsiPtr, locateQuat);
        }
    }

    if ((g_finalCameraHits % 600) != 1) return;

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

    if ((g_pitchHookHits % 600) == 1) {
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

    if (cameraState && desiredFov > 1.0f) {
        const uintptr_t stateAddr = reinterpret_cast<uintptr_t>(cameraState);
        WriteFloatSafe(stateAddr + 0x414, g_normalFovOverrideValue);
    }

    if ((g_normalFovHookHits % 600) == 1) {
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
        Log("MenuMode hook: state=%p prev=%d new=%d\n", menuState, prevMode, newMode);
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

extern "C" void __fastcall OnOnFootDeltaHeadCallback(float* deltaHead) {
    // Delta head contains pitch and yaw deltas
    // We can zero them out to decouple head from body!
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

extern "C" void __fastcall OnDLSSMatricesCallback(void* callThis, void* matrixState, uint32_t matrixSlot) {
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
        return;
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
        return;
    }

    const uintptr_t returnAddr = g_dlssMatricesHookSite ? g_dlssMatricesHookSite + 14 : 0;
    const unsigned long long siteRva = IsInGameModule(g_dlssMatricesHookSite)
        ? static_cast<unsigned long long>(g_dlssMatricesHookSite - g_gameModuleBase)
        : 0ULL;
    const unsigned long long targetRva = IsInGameModule(g_dlssMatricesCallTarget)
        ? static_cast<unsigned long long>(g_dlssMatricesCallTarget - g_gameModuleBase)
        : 0ULL;

    Log("DLSSMatrices hook: hit=%llu site=%p(rva=0x%llX) target=%p(rva=0x%llX) return=%p this=%p state=%p slot=%u adjusted=%u eye=%d mode=%d\n",
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
        return;
    }

    LogDLSSMatricesStateWindow(state);
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

    // Original instruction first so the struct already has the new XY scalar.
    code[pos++] = 0xF3; code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x86;
    code[pos++] = 0x94; code[pos++] = 0x00; code[pos++] = 0x00; code[pos++] = 0x00;

    code[pos++] = 0x50;
    WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(g_telemetry) + kMoveXYTelemetryOffset);
    code[pos++] = 0xFF; code[pos++] = 0x00;
    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0x70; code[pos++] = 0x08;
    code[pos++] = 0xF3; code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x40; code[pos++] = 0x10;
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

DWORD WINAPI WorkerThread(LPVOID) {
    Log("Worker thread started, waiting 8 seconds...\n");
    if (g_backendModulePath[0] != '\0') {
        Log("Backend module loaded from: %s\n", g_backendModulePath);
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

    Log("Starting LocateCamera probe hook installation...\n");
    bool h1 = InstallLocateCameraHook();
    Log("LocateCamera hook result: %s\n", h1 ? "SUCCESS" : "FAILED");

    Log("Starting PatchCamera probe hook installation...\n");
    bool h2 = InstallPatchCameraHook();
    Log("PatchCamera hook result: %s\n", h2 ? "SUCCESS" : "FAILED");

    Log("Starting FinalCamera probe hook installation...\n");
    bool h3 = InstallFinalCameraHook();
    Log("FinalCamera hook result: %s\n", h3 ? "SUCCESS" : "FAILED");

    Log("Starting Pitch hook installation...\n");
    bool h_pitch = InstallPitchHook();
    g_pitchHookInstalled = h_pitch;
    Log("Pitch hook result: %s\n", h_pitch ? "SUCCESS" : "FAILED");

    Log("Starting NormalFOV hook installation...\n");
    bool h_fov = InstallNormalFovHook();
    g_normalFovHookInstalled = h_fov;
    Log("NormalFOV hook result: %s\n", h_fov ? "SUCCESS" : "FAILED");

    Log("Starting FixLoD hook installation...\n");
    bool h_lod = InstallFixLoDHook();
    Log("FixLoD hook result: %s\n", h_lod ? "SUCCESS" : "FAILED");

    Log("Starting MenuMode hook installation...\n");
    bool h_menu = InstallMenuModeHook();
    Log("MenuMode hook result: %s\n", h_menu ? "SUCCESS" : "FAILED");

    Log("Starting ForceHeadingUpdate hook installation...\n");
    bool h_heading_force = InstallForceHeadingUpdateHook();
    g_forceHeadingUpdateHookInstalled = h_heading_force;
    Log("ForceHeadingUpdate hook result: %s\n", h_heading_force ? "SUCCESS" : "FAILED");

    Log("Starting OnFootDeltaHead probe hook installation...\n");
    bool h4 = InstallOnFootDeltaHeadHook();
    Log("OnFootDeltaHead hook result: %s\n", h4 ? "SUCCESS" : "FAILED");

    Log("Starting OnFootMoveXY probe hook installation...\n");
    bool h5 = InstallOnFootMoveXYHook();
    Log("OnFootMoveXY hook result: %s\n", h5 ? "SUCCESS" : "FAILED");

    Log("Starting FreeDeltaHead probe hook installation...\n");
    bool h6 = InstallFreeDeltaHeadHook();
    Log("FreeDeltaHead hook result: %s\n", h6 ? "SUCCESS" : "FAILED");

    Log("Starting PatchBuffer tracer hook installation...\n");
    bool h_pb = InstallPatchBufferHook();
    Log("PatchBuffer hook result: %s\n", h_pb ? "SUCCESS" : "FAILED");

    Log("Starting SettingsRes hook installation...\n");
    bool h10 = InstallSettingsResHook();
    Log("SettingsRes hook result: %s\n", h10 ? "SUCCESS" : "FAILED");

    Log("Starting DLSSRes hook installation...\n");
    bool h11 = InstallDLSSResHook();
    Log("DLSSRes hook result: %s\n", h11 ? "SUCCESS" : "FAILED");

    Log("Starting DLSSMatrices telemetry hook installation...\n");
    bool h12 = InstallDLSSMatricesHook();
    Log("DLSSMatrices hook result: %s\n", h12 ? "SUCCESS" : "FAILED");

    Log("Starting NativeSetterMetaWrite tracer hook installation...\n");
    bool h7 = InstallNativeSetterMetaWriteHook();
    Log("NativeSetterMetaWrite hook result: %s\n", h7 ? "SUCCESS" : "FAILED");

    Log("Starting NativeSetterMetaConsume tracer hook installation...\n");
    bool h8 = InstallNativeSetterMetaConsumeHook();
    Log("NativeSetterMetaConsume hook result: %s\n", h8 ? "SUCCESS" : "FAILED");

    Log("Starting NativeSetterClear tracer hook installation...\n");
    bool h9 = InstallNativeSetterClearHook();
    Log("NativeSetterClear hook result: %s\n", h9 ? "SUCCESS" : "FAILED");

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
    OpenXRManager::Get().Init();
}

using PFN_CreateDXGIFactory_Proxy = HRESULT(WINAPI*)(REFIID, void**);
using PFN_CreateDXGIFactory2_Proxy = HRESULT(WINAPI*)(UINT, REFIID, void**);
using PFN_DXGIDeclareAdapterRemovalSupport_Proxy = HRESULT(WINAPI*)();
using PFN_DXGIDisableVBlankVirtualization_Proxy = HRESULT(WINAPI*)();
using PFN_DXGIGetDebugInterface1_Proxy = HRESULT(WINAPI*)(UINT, REFIID, void**);
using PFN_DXGIReportAdapterConfiguration_Proxy = HRESULT(WINAPI*)(DWORD);

extern "C" __declspec(dllexport) HRESULT WINAPI CreateDXGIFactory(REFIID riid, void** ppFactory) {
    InitOpenXREarly();
    PrepareStartupLiveControls();
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
    InitOpenXREarly();
    PrepareStartupLiveControls();
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
    InitOpenXREarly();
    PrepareStartupLiveControls();
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
