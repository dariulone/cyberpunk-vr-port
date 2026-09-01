#pragma once

// windows.h, because some of what crosses this boundary is declared in Windows' own types (LONG on
// an interlocked counter, for one). Without it, a translation unit that includes this header FIRST
// fails to parse the declaration and then reports the error inside winnt.h, hundreds of lines away
// from the cause.
#include <windows.h>

#include <atomic>
#include <cstdint>

// ================================================================================================
// What a hook file needs from the core hub, and nothing else.
//
// This header exists so a hook can move into its own translation unit without dragging the hub
// along. It deliberately declares only what hooks actually read or write across the boundary --
// if this file starts growing a member for every global in VrCore.cpp, the split has failed and
// the monolith has merely been spread out.
//
// EVERY GLOBAL DECLARED HERE HAS EXTERNAL LINKAGE ON PURPOSE. Several are handed to
// WriteMovRaxImm64 at install time, i.e. their ADDRESS is baked into a patch as an imm64. Turn one
// into a file-static, a function-local static or a thread_local during a later tidy-up and it
// still compiles, still links, and the patch reads the wrong memory for the rest of the process's
// life. There is no diagnostic for that; this note is the diagnostic.
// ================================================================================================

// The project logger. Declared by hand in every module -- there is no logging header, and adding
// one is a separate change from moving the hooks.
extern void Log(const char* fmt, ...);

// Set from the launcher's DEBUG box. Per-frame chatter is gated on it.
extern volatile int g_verboseLog;

// The VERTICAL field of view written into the engine's camera field, in degrees. Hooks read it to
// tell the player's view apart from shadow-map and reflection views, which arrive at the same
// sites with their own FOVs.
extern volatile float g_normalFovOverrideValue;

// The engine's menu state, as the menu-mode hook last saw it. Shared because the hub answers a
// getter from it and the whole VR path branches on it.
extern volatile int g_menuModeValue;

// The XR runtime's RECOMMENDED render size. Named "forced" in one log line for historical reasons
// and it has already been misread once as evidence that something overrides the game's resolution:
// it does not. Nothing here writes the game's settings.
unsigned int GetForcedRenderWidthValue();
unsigned int GetForcedRenderHeightValue();
void ApplySettingsResolutionOverride(uintptr_t settings);

// The last settings / DLSS objects the engine handed us. Shared: the hooks write them, the hub's
// resolution logic reads them.
extern volatile uintptr_t g_settingsResPtr;
extern volatile uintptr_t g_dlssResPtr;

// Player state the movement hooks branch on, refreshed from RTTI once per frame by the hub.
extern bool g_isAiming;
extern bool g_hasWeaponEquipped;

// The render size the launcher was told to use. The FOV hooks derive the vertical from the
// horizontal through this aspect, so the two must be the SAME numbers the swapchain gets.
extern int g_launcherWidth;
extern int g_launcherHeight;

// The horizontal FOV the engine therefore renders, published for the submit and the overlay.
extern volatile float g_engineHorizontalFovDeg;

// xr_force_fov, or 0 to derive from the runtime.
extern "C" float GetForcedFov();

// Still read by the hub as well as by the hook that writes it.
extern void* volatile g_dbgFovCamState;  // 1 further use(s) in VrCore.cpp
extern volatile LONG g_pendingSnapYawDeltaBits;  // 1 further use(s) in VrCore.cpp
extern float g_projStageFov;  // 1 further use(s) in VrCore.cpp
extern float g_projStageAspect;  // 1 further use(s) in VrCore.cpp
extern uint64_t g_projStageHits;  // 1 further use(s) in VrCore.cpp
extern bool g_projStagePatched;  // 1 further use(s) in VrCore.cpp

// Written by Hooks/CameraFov.cpp, read by the hub's FOV diagnostic.
extern volatile float g_dbgLastOriginalFov;

// Player state the hub refreshes from RTTI once per frame.
extern bool g_isInVehicle;
// MOUNTED IS NOT DRIVING. g_isInVehicle (and shared[31]) mean "mounted to anything", passenger seats
// included; this one means the DRIVER seat, from VehicleComponent::IsDriver. The wheel grab needs the
// distinction because it hands the arms back to the DRIVING animation, which is the wrong pose in
// every other seat -- there is no wheel in the back of a taxi. A plain global, not a shared slot:
// producer (the camera hook) and consumers (the pose hook, the XInput merge) are all this DLL.
// Atomic because those are three different threads.
extern std::atomic<bool> g_isDriving;

// A UI POPUP THAT OWNS THE B BUTTON -- the phone, the radio port, the vehicle list. Published by the
// CET side (VRUiPopup), because none of the three is a menu: they are overlays, so the game's menu-mode
// value stays 0 and every gameplay rule still applies to the pad. The plugin needs to know for two
// opposite reasons: B must reach the game (it is Exit_Button there) and B must NOT reach the physical
// reload (a magazine drop while closing a popup is the accident this exists to prevent).
extern std::atomic<int> g_uiPopupOpen;
// When that flag last went down, so the magazine drop can stay blocked for a moment afterwards.
extern std::atomic<unsigned long long> g_uiPopupClosedMs;

// Which float in the engine's deltaHead[] receives the snap-turn yaw. A setting, because the slot
// is not the same in every build.
extern "C" int GetSnapTurnYawIndex();

// The shared-memory block the CET mods and the natives talk through; null until it is mapped.
float* GetShotShared();

extern float g_projStageExtra;
extern uint64_t g_unifixHits;
extern volatile uintptr_t g_unifixRenderObj;
extern float g_unifixProjDump[9];

// Whether the stereo module is loaded, and which view it says is recording. Declared here because
// the hub and Hooks/ViewKey.cpp both need the answer and neither owns it: the module defines the
// flag, and ViewKey defers to it rather than hooking the same dispatcher twice.
extern "C" __declspec(dllexport) extern int CyberpunkVR_StereoModuleLoaded;
extern "C" __declspec(dllexport) int CyberpunkVR_IsMainViewActive();
