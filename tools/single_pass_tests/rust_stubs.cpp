// The C++ half's callbacks the Rust library (cvr_rust.lib) can reach that no single-pass test exercises; the plugin
// defines them (src/Core/LiveControlsParse.cpp). Linked into every Rust variant through cvr_rust's interface.
extern "C" bool cvr_cpp_settings_parse_subsystem(int, const char*, void*) { return false; }
// The plugin's exports the Rust library reads or publishes (the plugin's C++ half defines them; no single-pass test reads
// them), and the scene access trace's descriptor cookie callback (src/Render/NativeSceneAccess.cpp).
#include <cstdint>
extern "C" {
int32_t CyberpunkVR_EngineStereoEyeWindows{},CyberpunkVR_MonoView{},CyberpunkVR_AsymSquarePixels{};
float CyberpunkVR_AsymInnerGuard{};
uint64_t CyberpunkVR_SceneAccessChildren{},CyberpunkVR_SceneAccessEvents{},CyberpunkVR_SceneAccessRecordings{},CyberpunkVR_SceneAccessResources{};
uint64_t CyberpunkVR_SceneAccessStats[16]{},CyberpunkVR_TargetAccessStats[16]{},CyberpunkVR_TargetAccessSources[32]{},CyberpunkVR_TargetAccessChecks[32]{};
uint64_t CyberpunkVR_TargetAccessBreadcrumbStats[8]{},CyberpunkVR_TargetAccessFailureEvent[8]{};
uint64_t CyberpunkVR_TargetHandoffStats[16]{},CyberpunkVR_TargetHandoffOwners[16]{},CyberpunkVR_TargetHandoffSourceLocal[16]{};
uint64_t CyberpunkVR_TargetHandoffSources[32]{},CyberpunkVR_TargetHandoffStates[80]{},CyberpunkVR_TargetHandoffTargets[192]{};
bool cvr_cpp_held_resource_cookie(void*,uint64_t*){return false;}
}