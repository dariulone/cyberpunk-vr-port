#include "Camera/CameraState.hpp"
#include "Camera/ExternalView.hpp"
#include "Camera/ExternalViewMath.hpp"
#include "Camera/PlayerCameraState.hpp"
#include "Core/VrCoreShared.hpp"
#include "Hooks/CameraPoseSites.hpp"
#include "Hooks/Hook.hpp"
#include "Overlay/ImGuiOverlay.hpp"
#include "Utils/DebugGate.hpp"
#include "Utils/MemorySafe.hpp"
#include "Utils/ThreadScope.hpp"
#include <RED4ext/RED4ext.hpp>
#include <MinHook.h>
#include <intrin.h>
#include <cstdio>
#include <cstring>

#include "Hooks/PanzerState.hpp"

#if CVR_RUST_VRIK
// rust/crates/cvr-rig/src/vrik/panzer.rs: the update, the two getters and the launch.
extern "C" void cvr_panzer_bind(uintptr_t,uintptr_t,uintptr_t,uintptr_t,uintptr_t,uintptr_t,bool,uintptr_t,uintptr_t);
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
extern "C" void CvrPanzerBind(uintptr_t exe,uintptr_t steer,uintptr_t update,uintptr_t position,uintptr_t rotation,uintptr_t launch,
                              bool ready,uintptr_t mainAim,uintptr_t externalDirection) {
    cvr_panzer_bind(exe,steer,update,position,rotation,launch,ready,mainAim,externalDirection);
}
#endif
#if !CVR_RUST_VRIK   // registered from rust/crates/cvr-render/src/hook_table.rs
CVR_HOOK("PanzerWeaponAim",::cvr::hooks::Stage::Boot,84,cvr_hook_panzer_weapon_aim_install);
#endif
#else
// The player's tank by RTTI, through src/Hooks/HookServices.cpp, as the Rust port asks it.
extern "C" bool cvr_cpp_is_tank(uintptr_t owner);
void CvrPanzerSteeringBind(uintptr_t steer,uintptr_t externalDirection);   // src/Hooks/PanzerSteering.cpp
namespace {
using UpdateFn=uintptr_t(__fastcall*)(uintptr_t);
using GetFn=float*(__fastcall*)(uintptr_t,float*);
using LaunchFn=uintptr_t(__fastcall*)(uintptr_t,uintptr_t,const float*,const float*,const float*,float,uint8_t);
UpdateFn originalUpdate{};
LaunchFn originalLaunch{};
GetFn originalPosition{},originalRotation{};
uintptr_t exe{};
// The vehicle's weapon slots (DynArray of 104-byte entries, the weapon at +0x10) and its aim target. 2.5 moved them
// 0x20 down the vehicle (2.31: +0xB00 / +0xB0C / +0xB14): sub_1409E6B1C walks +0xB20 by +0xB2C, sub_1407E351C
// writes the target at +0xB34. With the 2.31 offsets every launch failed the slot check and stayed native.
constexpr uintptr_t kWeaponSlots=0xB20,kWeaponSlotCount=0xB2C,kAimTarget=0xB34;
std::mutex debugMutex;
std::atomic<bool> hooksReady{false};
using MainAimFn=bool(*)(cvr::camera::MainAimPose*);
MainAimFn readMainAim{};
bool ReadMainAim(cvr::camera::MainAimPose& pose) { return readMainAim ? readMainAim(&pose):cvr::camera::ReadMainAimPose(pose); }
struct Context {
    cvr::camera::MainAimPose pose;
    PanzerWeaponAimDebug debug;
    bool positionWritten{},rotationWritten{};
};
using Scope=cvr::ThreadScope<Context>;

bool IsPlayerTank(uintptr_t owner) {
    if(!hooksReady.load(std::memory_order_acquire) || !owner || !g_isDriving.load(std::memory_order_relaxed) ||
       g_menuModeValue!=0 || g_sceneTier.load(std::memory_order_relaxed)>2 || OverlayIsVisible() ||
       owner!=CurrentMountedVehicleIdentity())return false;
    return cvr_cpp_is_tank(owner);
}
}

extern "C" uintptr_t CvrPanzerLaunchAt(uintptr_t owner,uintptr_t weapon,const float* position,const float* forward,
                                       const float* target,float spread,uint8_t count,uintptr_t ret) {
    uintptr_t slots{},cannon{};uint32_t size{};
    if(ret!=exe+cvr::camera::sites::VehicleWeaponTargetedLaunchCall.rva+5 ||
       !IsPlayerTank(owner) || !position || !forward || !target ||
       !ReadU64Safe(owner+kWeaponSlots,&slots) || !slots || !ReadU32Safe(owner+kWeaponSlotCount,&size) || size<1 || size>8 ||
       !ReadU64Safe(slots+0x10,&cannon) || cannon!=weapon)
        return originalLaunch(owner,weapon,position,forward,target,spread,count);
    // The native cannon launch otherwise constructs its direction provider from
    // muzzle+barrelForward, even though the vehicle has a camera-derived target.
    // Converge from the native muzzle onto that collision-tested point. Slot 0
    // is the cannon; missile launch arcs and radial countermeasures stay native.
    cvr::camera::MainAimPose current{};XrVector3f direction{};
    const bool apply=ReadMainAim(current) &&
        cvr::camera::AimAtPoint({position[0],position[1],position[2]}, {target[0],target[1],target[2]},direction);
    const float redirected[]{direction.x,direction.y,direction.z};
    const auto result=originalLaunch(owner,weapon,position,apply?redirected:forward,target,spread,count);
    if(cvr::RuntimeDiagnosticsEnabled()) {
        PanzerCannonDebug next{};next.owner=owner;next.currentMainPoseId=current.poseId;next.stampUs=XrDiagNowUs();next.reason=apply?0:1;
        std::memcpy(next.position,position,12);std::memcpy(next.nativeForward,forward,12);
        std::memcpy(next.forward,apply?redirected:forward,12);std::memcpy(next.target,target,12);
        std::lock_guard lock(debugMutex);
        next.calls=CyberpunkVR_PanzerCannonDebug.calls+1;next.applied=CyberpunkVR_PanzerCannonDebug.applied+apply;
        CyberpunkVR_PanzerCannonDebugSeq.fetch_add(1,std::memory_order_acq_rel);
        CyberpunkVR_PanzerCannonDebug=next;
        CyberpunkVR_PanzerCannonDebugSeq.fetch_add(1,std::memory_order_release);
    }
    return result;
}

extern "C" float* CvrPanzerPositionAt(uintptr_t manager,float* out,uintptr_t ret) {
    auto* result=originalPosition(manager,out);
    if(auto* scope=Scope::Get();scope && result==out && out &&
       ret==exe+cvr::camera::sites::PanzerWeaponPositionCall.rva+5) {
        CVR_DIAGNOSTIC(std::memcpy(scope->debug.nativePosition,out,12));
        std::memcpy(out,scope->pose.position,12);scope->positionWritten=true;
    }
    return result;
}
extern "C" float* CvrPanzerRotationAt(uintptr_t manager,float* out,uintptr_t ret) {
    auto* result=originalRotation(manager,out);
    if(auto* scope=Scope::Get();scope && result==out && out &&
       ret==exe+cvr::camera::sites::PanzerWeaponRotationCall.rva+5) {
        CVR_DIAGNOSTIC(std::memcpy(scope->debug.nativeRotation,out,16));
        std::memcpy(out,scope->pose.rotation,16);scope->rotationWritten=true;
    }
    return result;
}
namespace {
void Record(Context& frame,uintptr_t owner) {
    auto next=frame.debug;next.owner=owner;next.poseId=frame.pose.poseId;next.stampUs=XrDiagNowUs();
    std::memcpy(next.position,frame.pose.position,12);std::memcpy(next.rotation,frame.pose.rotation,16);
    if(frame.positionWritten && frame.rotationWritten) {
        next.reason=0;
        ReadFloatArraySafe(reinterpret_cast<const float*>(owner+kAimTarget),next.target,3);
    }
    std::lock_guard lock(debugMutex);
    next.calls=CyberpunkVR_PanzerWeaponAimDebug.calls+1;
    next.applied=CyberpunkVR_PanzerWeaponAimDebug.applied+(next.reason==0);
    next.positionWrites=CyberpunkVR_PanzerWeaponAimDebug.positionWrites+frame.positionWritten;
    next.rotationWrites=CyberpunkVR_PanzerWeaponAimDebug.rotationWrites+frame.rotationWritten;
    CyberpunkVR_PanzerWeaponAimDebugSeq.fetch_add(1,std::memory_order_acq_rel);
    CyberpunkVR_PanzerWeaponAimDebug=next;
    CyberpunkVR_PanzerWeaponAimDebugSeq.fetch_add(1,std::memory_order_release);
}
}
extern "C" uintptr_t CvrPanzerAimUpdate(uintptr_t owner) {
    Context frame{};frame.debug.reason=1;bool apply=false;
    do {
        // The callback owns its object. A remembered vehicle address is used
        // only for comparison, never to find/dereference a cached entity.
        if(!IsPlayerTank(owner))break;
        frame.debug.reason=3;
        if(!ReadMainAim(frame.pose))break;
        frame.debug.reason=4;apply=true;
    } while(false);
    Scope scope(apply?&frame:nullptr);
    // The native function performs its original collision query and updates the
    // weapon target at +B14, including the missile salvo's original latch/spread.
    // Only its two camera getter results change; no shared camera is modified.
    const auto result=originalUpdate(owner);
    CVR_DIAGNOSTIC(Record(frame,owner));
    return result;
}
extern "C" void CvrPanzerBind(uintptr_t exe_,uintptr_t steer,uintptr_t update,uintptr_t position,uintptr_t rotation,uintptr_t launch,
                              bool ready,uintptr_t mainAim,uintptr_t externalDirection) {
    exe=exe_;originalUpdate=reinterpret_cast<UpdateFn>(update);originalPosition=reinterpret_cast<GetFn>(position);
    originalRotation=reinterpret_cast<GetFn>(rotation);originalLaunch=reinterpret_cast<LaunchFn>(launch);
    readMainAim=reinterpret_cast<MainAimFn>(mainAim);hooksReady.store(ready,std::memory_order_release);
    CvrPanzerSteeringBind(steer,externalDirection);
}
namespace {
// The detours: the getters and the launch ask where they were called from.
float* __fastcall Position(uintptr_t manager,float* out) {
    return CvrPanzerPositionAt(manager,out,reinterpret_cast<uintptr_t>(_ReturnAddress()));
}
float* __fastcall Rotation(uintptr_t manager,float* out) {
    return CvrPanzerRotationAt(manager,out,reinterpret_cast<uintptr_t>(_ReturnAddress()));
}
uintptr_t __fastcall Launch(uintptr_t owner,uintptr_t weapon,const float* position,const float* forward,
                            const float* target,float spread,uint8_t count) {
    return CvrPanzerLaunchAt(owner,weapon,position,forward,target,spread,count,reinterpret_cast<uintptr_t>(_ReturnAddress()));
}
bool Matches(const cvr::camera::sites::Site& site) {
    const auto* bytes=reinterpret_cast<const uint8_t*>(exe+site.rva);
    for(size_t i=0;site.bytes[i*2];++i) {
        unsigned value{};
        if(std::sscanf(site.bytes+i*2,"%2x",&value)!=1 || bytes[i]!=value)return false;
    }
    return true;
}
bool Install() {
    exe=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    using namespace cvr::camera::sites;
    const Site verified[]{PanzerWeaponAim,VehicleCameraPosition,VehicleCameraRotation,PanzerWeaponPositionCall,PanzerWeaponRotationCall,VehicleWeaponLaunch,VehicleWeaponTargetedLaunchCall};
    for(const auto& site:verified)if(!Matches(site))return false;
    struct Hook {uintptr_t rva;void* detour;void** original;};
    const Hook hooks[]{
        {VehicleCameraPosition.rva,reinterpret_cast<void*>(&Position),reinterpret_cast<void**>(&originalPosition)},
        {VehicleCameraRotation.rva,reinterpret_cast<void*>(&Rotation),reinterpret_cast<void**>(&originalRotation)},
        {PanzerWeaponAim.rva,reinterpret_cast<void*>(&CvrPanzerAimUpdate),reinterpret_cast<void**>(&originalUpdate)},
        {VehicleWeaponLaunch.rva,reinterpret_cast<void*>(&Launch),reinterpret_cast<void**>(&originalLaunch)}};
    for(const auto& hook:hooks) {
        auto* target=reinterpret_cast<void*>(exe+hook.rva);
        if(MH_CreateHook(target,hook.detour,hook.original)!=MH_OK || MH_EnableHook(target)!=MH_OK)return false;
    }
    hooksReady.store(true,std::memory_order_release);return true;
}
CVR_HOOK("PanzerWeaponAim",::cvr::hooks::Stage::Boot,84,Install);
}
#endif
