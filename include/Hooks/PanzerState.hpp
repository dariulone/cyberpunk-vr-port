#pragma once
// The tank hooks' debug records (src/Hooks/PanzerSteering.cpp and PanzerWeaponAim.cpp, or
// rust/crates/cvr-rig/src/vrik/panzer.rs), exported from src/Hooks/PanzerState.cpp in both builds. Each is seqlocked
// by its *Seq counter: odd while it is written.
#include <atomic>
#include <cstdint>

struct PanzerSteeringDebug {
    uint64_t calls{},applied{},model{},owner{},stampUs{},poseId{};
    float nativeTarget[3]{},target[3]{};
    uint32_t reason{},reserved{};
};
static_assert(sizeof(PanzerSteeringDebug)==80);
struct PanzerWeaponAimDebug {
    uint64_t calls{},applied{},positionWrites{},rotationWrites{},owner{},poseId{},stampUs{};
    float nativePosition[3]{},nativeRotation[4]{},position[3]{},rotation[4]{},target[3]{};
    uint32_t reason{};
};
static_assert(sizeof(PanzerWeaponAimDebug)==128);
struct PanzerCannonDebug {
    uint64_t calls{},applied{},owner{},currentMainPoseId{},stampUs{};
    float position[3]{},nativeForward[3]{},forward[3]{},target[3]{};
    uint32_t reason{};
};
static_assert(sizeof(PanzerCannonDebug)==96);
extern "C" {
__declspec(dllexport) extern int CyberpunkVR_PanzerGazeSteering;
__declspec(dllexport) extern PanzerSteeringDebug CyberpunkVR_PanzerSteeringDebug;
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_PanzerSteeringDebugSeq;
__declspec(dllexport) extern PanzerWeaponAimDebug CyberpunkVR_PanzerWeaponAimDebug;
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_PanzerWeaponAimDebugSeq;
__declspec(dllexport) extern PanzerCannonDebug CyberpunkVR_PanzerCannonDebug;
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_PanzerCannonDebugSeq;
// For tools/vrik_tests: the image the call sites are measured in, the five originals, the weapon-aim gate and the two
// readers of the main view (null: src/Hooks/CameraPoseTransport.cpp's).
void CvrPanzerBind(uintptr_t exe,uintptr_t steer,uintptr_t update,uintptr_t position,uintptr_t rotation,uintptr_t launch,
                   bool ready,uintptr_t mainAim,uintptr_t externalDirection);
// The detours' bodies, the return address as one more argument where they check it.
uintptr_t CvrPanzerSteerTarget(uintptr_t model);
uintptr_t CvrPanzerAimUpdate(uintptr_t owner);
float* CvrPanzerPositionAt(uintptr_t manager,float* out,uintptr_t ret);
float* CvrPanzerRotationAt(uintptr_t manager,float* out,uintptr_t ret);
uintptr_t CvrPanzerLaunchAt(uintptr_t owner,uintptr_t weapon,const float* position,const float* forward,const float* target,
                            float spread,uint8_t count,uintptr_t ret);
}
