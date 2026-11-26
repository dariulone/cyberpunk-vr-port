#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
// WeaponAim's exports (src/Hooks/WeaponAim.cpp, or rust/crates/cvr-rig/src/vrik/weapon_aim.rs): the switches and the
// diagnostics of the hitscan from the muzzle, the gaze-centred targeting and the install state. Defined here so the
// C++ and the Rust build -- and tools/vrik_tests -- export the same names.
#if !CVR_RUST_VRIK  // the Rust build's are rust/crates/cvr-rig/src/vrik/state_globals.rs
#include <cstdint>

extern "C" __declspec(dllexport) int CyberpunkVR_DebugWaInstalled = 0;
extern "C" __declspec(dllexport) uintptr_t CyberpunkVR_GazeTargetState = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_GazeTargetUpdates = 0;
extern "C" __declspec(dllexport) int CyberpunkVR_HitscanFromMuzzle = 1;
extern "C" __declspec(dllexport) int CyberpunkVR_DebugPhysicalRayPatched = 0;
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugPhysicalRayCalls = 0;
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugPhysicalRayMine = 0;
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugPhysicalRayOriginWrites = 0;
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugPhysicalRayForwardWrites = 0;
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugPhysicalRayConeRays = 0;
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugPhysicalRayRecoil = 0;
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugPhysicalRayForeign = 0;
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugPhysicalRayLeftGun = 0;
extern "C" __declspec(dllexport) float CyberpunkVR_DebugDualShot[4] = {};
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugDualShotRecord[3] = {};
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugPhysicalRayStaleMuzzle = 0;
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugPhysicalRaySightLine = 0;
extern "C" __declspec(dllexport) float CyberpunkVR_DebugPhysicalRaySightLift[4] = {};
extern "C" __declspec(dllexport) float CyberpunkVR_DebugPhysicalRayRawForward[4] = {};
extern "C" __declspec(dllexport) float CyberpunkVR_DebugPhysicalRayOutForward[4] = {};
extern "C" __declspec(dllexport) int32_t CyberpunkVR_ConeCentreFromCamera = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugConeFirstRay = 0;
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugPhysicalRayShared = 0;
extern "C" __declspec(dllexport) int CyberpunkVR_HitscanSharedRay = 1;
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugPhysicalRaySharedMissing = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPhysicalRayKeys[64] = {};
#endif
#endif
