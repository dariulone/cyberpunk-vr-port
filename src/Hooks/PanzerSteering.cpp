#if !CVR_RUST_VRIK   // the Rust build registers this file's hook from rust/crates/cvr-render/src/hook_table.rs
#include "Camera/CameraState.hpp"
#include "Camera/ExternalView.hpp"
#include "Camera/PlayerCameraState.hpp"
#include "Core/VrCoreShared.hpp"
#include "Hooks/CameraPoseSites.hpp"
#include "Hooks/Hook.hpp"
#include "Hooks/RoomscaleMove.hpp"
#include "Overlay/ImGuiOverlay.hpp"
#include "Utils/DebugGate.hpp"
#include "Utils/MemorySafe.hpp"
#include <MinHook.h>
#include <cstdio>
#include <cstring>

#include "Hooks/PanzerState.hpp"

#if CVR_RUST_VRIK
// rust/crates/cvr-rig/src/vrik/panzer.rs: the steering target.
extern "C" bool cvr_hook_panzer_steering_install();
CVR_HOOK("PanzerSteering",::cvr::hooks::Stage::Boot,83,cvr_hook_panzer_steering_install);
#else
namespace {
using TargetFn=uintptr_t(__fastcall*)(uintptr_t);
TargetFn original{};
std::mutex debugMutex;
using DirectionFn=bool(*)(float*,uint64_t*);
DirectionFn readDirection{};
uintptr_t MountedVehicle() {
    return CurrentMountedVehicleIdentity();
}
void Record(const PanzerSteeringDebug& frame) {
    std::lock_guard lock(debugMutex);auto next=frame;
    next.calls=CyberpunkVR_PanzerSteeringDebug.calls+1;
    next.applied=CyberpunkVR_PanzerSteeringDebug.applied+(frame.reason==0);
    CyberpunkVR_PanzerSteeringDebugSeq.fetch_add(1,std::memory_order_acq_rel);
    CyberpunkVR_PanzerSteeringDebug=next;
    CyberpunkVR_PanzerSteeringDebugSeq.fetch_add(1,std::memory_order_release);
}
}
// The bench's originals and reader (CvrPanzerBind, src/Hooks/PanzerWeaponAim.cpp).
void CvrPanzerSteeringBind(uintptr_t steer,uintptr_t externalDirection) {
    original=reinterpret_cast<TargetFn>(steer);readDirection=reinterpret_cast<DirectionFn>(externalDirection);
}
extern "C" uintptr_t CvrPanzerSteerTarget(uintptr_t model) {
    const auto result=original(model);
    PanzerSteeringDebug frame{};frame.model=model;frame.reason=1;
    do {
        if(!CyberpunkVR_PanzerGazeSteering || !g_isDriving.load(std::memory_order_relaxed) ||
           g_menuModeValue!=0 || g_sceneTier.load(std::memory_order_relaxed)>2 || OverlayIsVisible())break;
        uintptr_t owner{};frame.reason=2;
        if(!ReadU64Safe(model+0xE0,&owner)||!owner || owner!=MountedVehicle())break;
        frame.owner=owner;frame.reason=3;
        if(!(readDirection ? readDirection(frame.target,&frame.poseId):cvr::camera::ReadExternalMainDirection(frame.target,&frame.poseId)))break;
        // CP2077 2.31: FixedUpdate_PreSolve calls this native TPP camera helper,
        // then compares chassis forward with model+E8/EC/F0 at +261D4B2..515.
        // Replace its planar target only. The native yaw controller, turning
        // speed, forces, acceleration and camera orbit remain owned by the game.
        CVR_DIAGNOSTIC(ReadFloatArraySafe(reinterpret_cast<const float*>(model+0xE8),frame.nativeTarget,3));
        frame.reason=4;
        if(!WriteFloatSafe(model+0xE8,frame.target[0]) || !WriteFloatSafe(model+0xEC,frame.target[1]) ||
           !WriteFloatSafe(model+0xF0,0))break;
        frame.reason=0;
    } while(false);
    CVR_DIAGNOSTIC(frame.stampUs=XrDiagNowUs();Record(frame));
    return result;
}
namespace {
bool Matches(uintptr_t base,const cvr::camera::sites::Site& site) {
    const auto* data=reinterpret_cast<const uint8_t*>(base+site.rva);
    for(size_t i=0;site.bytes[i*2];++i) {
        unsigned value{};
        if(std::sscanf(site.bytes+i*2,"%2x",&value)!=1 || data[i]!=value)return false;
    }
    return true;
}
bool Install() {
    const auto exe=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    if(!exe || !Matches(exe,cvr::camera::sites::PanzerAimUpdate)||!Matches(exe,cvr::camera::sites::PanzerAimCall))return false;
    auto* target=reinterpret_cast<void*>(exe+cvr::camera::sites::PanzerAimUpdate.rva);
    if(MH_CreateHook(target,reinterpret_cast<void*>(&CvrPanzerSteerTarget),reinterpret_cast<void**>(&original))!=MH_OK)return false;
    return MH_EnableHook(target)==MH_OK;
}
CVR_HOOK("PanzerSteering",::cvr::hooks::Stage::Boot,83,Install);
}
#endif
#endif
