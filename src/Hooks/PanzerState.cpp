#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
// The tank hooks' exports (src/Hooks/PanzerSteering.cpp and PanzerWeaponAim.cpp, or rust/crates/cvr-rig/src/vrik/panzer.rs):
// the gaze-steering switch and the three seqlocked debug records. Both builds, and tools/vrik_tests.
#if !CVR_RUST_VRIK  // the Rust build's are rust/crates/cvr-rig/src/vrik/state_globals.rs
#include "Hooks/PanzerState.hpp"

extern "C" {
__declspec(dllexport) int CyberpunkVR_PanzerGazeSteering=1;
__declspec(dllexport) PanzerSteeringDebug CyberpunkVR_PanzerSteeringDebug{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PanzerSteeringDebugSeq{};
__declspec(dllexport) PanzerWeaponAimDebug CyberpunkVR_PanzerWeaponAimDebug{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PanzerWeaponAimDebugSeq{};
__declspec(dllexport) PanzerCannonDebug CyberpunkVR_PanzerCannonDebug{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PanzerCannonDebugSeq{};
}
#endif
#endif
