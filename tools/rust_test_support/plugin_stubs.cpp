// The plugin's pieces a test program linked against cvr_rust.lib (without the plugin's features) does not have, for the
// Rust variants of the test programs: the exports the Rust half reads by name but the plugin defines elsewhere (in Rust's
// plugin-only modules or in C++ files the tests do not compile), with the plugin's starting values, and the C++ adapters it
// calls, standing in with "nothing known". None of them is exercised by the tests that link this file.
#include <atomic>
#include <cstdint>

extern "C" {
// Stereo/AsymProjection.cpp, Stereo/EngineStereo.cpp: the projection's switches.
__declspec(dllexport) float CyberpunkVR_AsymInnerGuard = 0.0f;
__declspec(dllexport) int32_t CyberpunkVR_AsymSquarePixels = 1;
__declspec(dllexport) int32_t CyberpunkVR_EngineStereoEyeWindows = 1;
__declspec(dllexport) int32_t CyberpunkVR_MonoView = 0;
// Render/NativeSceneAccess.cpp's rows.
__declspec(dllexport) uint64_t CyberpunkVR_SceneAccessEvents{}, CyberpunkVR_SceneAccessChildren{};
__declspec(dllexport) uint64_t CyberpunkVR_SceneAccessRecordings{}, CyberpunkVR_SceneAccessResources{};
__declspec(dllexport) uint64_t CyberpunkVR_SceneAccessStats[16]{};
// Render/NativeTargetAccess.cpp's rows.
__declspec(dllexport) uint64_t CyberpunkVR_TargetAccessBreadcrumbStats[8]{};
__declspec(dllexport) uint64_t CyberpunkVR_TargetAccessChecks[32]{};
__declspec(dllexport) uint64_t CyberpunkVR_TargetAccessFailureEvent[8]{};
__declspec(dllexport) uint64_t CyberpunkVR_TargetAccessSources[32]{};
__declspec(dllexport) uint64_t CyberpunkVR_TargetAccessStats[16]{};
// Render/NativeTargetHandoff.cpp's rows.
uint64_t CyberpunkVR_TargetHandoffStats[16]{2};
uint64_t CyberpunkVR_TargetHandoffSources[32]{};
uint64_t CyberpunkVR_TargetHandoffSourceLocal[16]{};
uint64_t CyberpunkVR_TargetHandoffOwners[16]{};
uint64_t CyberpunkVR_TargetHandoffTargets[192]{};
uint64_t CyberpunkVR_TargetHandoffStates[80]{};
// Render/StereoDescriptorAudit.cpp's held-resource cookie: none known.
bool cvr_cpp_held_resource_cookie(void*, uint64_t* cookie) {
    if (cookie) *cookie = 0;
    return false;
}
// Core/LiveControlsParse.cpp's subsystem settings parser: no line is a subsystem's.
bool cvr_cpp_settings_parse_subsystem(int, const char*, void*) { return false; }
}
