#pragma once

// ================================================================================================
// What the hub's files hand each other, after Core/VrCore.cpp was split by subject.
//
// The rule is the same one Stereo/StereoInternal.hpp carries: a name belongs here when a file OTHER
// than the one defining it uses that name. Nothing else. VrCore.cpp keeps the shared STATE and the
// entry points; this header is only the seam between the pieces that moved out.
//
// Declarators are copied from the definitions, never retyped.
// ================================================================================================

#include "Core/LiveControls.hpp"
#include "Core/VrCoreShared.hpp"
// LiveControlsUiState, which MakeLiveControlsUiState below returns BY VALUE. This header used to
// borrow it from whichever .cpp happened to include the overlay first, so it only compiled inside
// the two files that already did -- a natives file including it got a syntax error on line 19.
#include "Overlay/LiveControlsUi.hpp"

#include <windows.h>
#include <cstdint>
#include <cstddef>
LiveControlsUiState MakeLiveControlsUiState();
bool IsPlausiblePositionVec4(const float* v);
bool ReadFloatArraySafe(const float* src, float* out, size_t count);
bool WriteFloatArraySafe(float* dst, const float* values, size_t count);
bool WriteU32Safe(uintptr_t addr, uint32_t value);
extern FILE* g_logFile;
extern FILETIME g_lastLiveControlWrite;
extern FILETIME g_lastVrikRecenterWrite;
extern char g_backendModulePath[MAX_PATH];
extern char g_gameDir[MAX_PATH];
extern char g_launcherConfigPath[MAX_PATH];
extern char g_liveControlPath[MAX_PATH];
extern char g_vrikRecenterPath[MAX_PATH];
extern char g_vrikSettingsPath[MAX_PATH];
extern int g_lastVrikRecenterCounter;
extern int g_launcherDebug;
extern int g_launcherHmdType;
extern size_t g_gameModuleSize;
extern uintptr_t g_gameModuleBase;
float GetTargetRenderVfovDeg();
int ClampRuntimeMode(int value);
void ApplyKnownResolutionOverrides();
void EnsureLiveControlFileExists();
void InitGameModuleInfo();
void InitRuntimePaths();
void LoadLauncherConfig();
void LogFloatAt(const char* label, uintptr_t addr);
void LogPtrAt(const char* label, uintptr_t addr);
void LogPtrPayloadVec4At(const char* label, uintptr_t addr);
void LogStackWindowAt(const char* label, uintptr_t rsp, int slots);
void LogU8At(const char* label, uintptr_t addr);
void LogVec4At(const char* label, uintptr_t addr);
void PersistLiveControlsUiState(const LiveControlsUiState& state);
void PollHotkeys();
void PollLiveControls();
void SaveLauncherConfig(int width, int height);
void WriteVrikSettingsFile();



// Called from WorkerThread.cpp as well as from the plugin entry point. It is idempotent, and the
// call in the worker exists only to settle the ORDER between it and the first poll.
//
// extern "C" IS LOAD-BEARING. The definition in VrCore.cpp sits inside the `extern "C" {` block that
// wraps the old DXGI exports, so it has C linkage. Declaring it here without that would compile in
// isolation and fail with "linkage specification contradicts earlier specification" -- the same trap
// VrCore.cpp already documents next to its `extern "C++"` block for the cvr:: functions.
extern "C" void InitStereoOnce();