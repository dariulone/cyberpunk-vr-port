#if !CVR_RUST_VRIK   // the Rust build registers this file's hook from rust/crates/cvr-render/src/hook_table.rs
#include "Utils/DebugGate.hpp"
#include "Camera/CameraState.hpp"
#include "Camera/PoseIdentity.hpp"
#include "Hooks/CameraPoseSites.hpp"
#include "Hooks/Hook.hpp"
#include "Hooks/RoomscaleMove.hpp"
#include "Hooks/Trampoline.hpp"
#include "Utils/MemorySafe.hpp"
#include "Core/VrCoreShared.hpp"
#include <cstring>
#include <cstdio>

#if CVR_RUST_VRIK
// rust/crates/cvr-rig/src/vrik/world_blend.rs: CvrWorldBlendBeforeNotify (called from CameraPoseLeafThunks.asm) and the
// install, the same site and relay.
extern "C" bool cvr_hook_world_blend_install();
CVR_HOOK("WorldSpaceBlendCamera",::cvr::hooks::Stage::Boot,82,cvr_hook_world_blend_install);
#else
// The counters are defined in src/Camera/CameraLinkState.cpp (both builds); the reset, the origin and MAIN's
// camera are asked through the services, as the Rust port asks them.
extern "C" {
void CvrWorldBlendNotifyDetour();
extern std::atomic<uint64_t> CyberpunkVR_WorldBlendPoseCalls;
extern std::atomic<uint64_t> CyberpunkVR_WorldBlendPoseApplied;
extern std::atomic<uint64_t> CyberpunkVR_WorldBlendPoseMissing;
extern std::atomic<uint64_t> CyberpunkVR_WorldBlendPoseChanged;
bool cvr_cpp_xr_external_pose_reset_pending();
uint64_t cvr_cpp_xr_tracking_origin_serial();
uintptr_t cvr_cpp_cam_obj_main();
}

namespace {
bool WritePose(uintptr_t component,const cvr::camera::PoseFingerprint& pose) {
    __try {
        std::memcpy(reinterpret_cast<void*>(component+0xE0),pose.position.data(),12);
        std::memcpy(reinterpret_cast<void*>(component+0xF0),pose.rotation.data(),16);
        return true;
    } __except(EXCEPTION_EXECUTE_HANDLER) { return false; }
}
}

extern "C" void CvrWorldBlendBeforeNotify(uintptr_t component) {
    CVR_DIAGNOSTIC(++CyberpunkVR_WorldBlendPoseCalls);
    if(component<0x10000)return;
    cvr::camera::InvalidatePoseAddress(component+0xE0);
    if(g_menuModeValue!=0 || DeviceCamActive() || g_bdActive.load(std::memory_order_relaxed) ||
       cvr_cpp_xr_external_pose_reset_pending())return;
    uintptr_t source{},owner{},sourceOwner{};
    const auto player=cvr::roomscale::PlayerIdentity();
    if(!player || !ReadPtrSafe(component+0x1E0,&source) || !source ||
       source!=cvr_cpp_cam_obj_main() ||
       !ReadPtrSafe(component+0x50,&owner) || owner!=player ||
       !ReadPtrSafe(source+0x50,&sourceOwner) || sourceOwner!=player)return;

    // The verified native update reads this exact source before smoothing it.
    // It already contains the shared VR composition and MAIN's eye offset.
    // Copy that completed write; do not locate a newer head or add HMD twice.
    const auto receipt=cvr::camera::CapturePoseAddress(source+0xE0);
    if(!receipt || receipt.payload.component!=source || receipt.payload.view!=1 ||
       receipt.payload.head.originSerial!=cvr_cpp_xr_tracking_origin_serial()) {
        CVR_DIAGNOSTIC(++CyberpunkVR_WorldBlendPoseMissing);return;
    }
    if(!WritePose(component,receipt.fingerprint))return;
    cvr::camera::PublishSerializedPose(component+0xE0,receipt);
    const auto written=cvr::camera::CapturePoseAddress(component+0xE0);
    if(written && written.payload.writeId==receipt.payload.writeId)
        CVR_DIAGNOSTIC(++CyberpunkVR_WorldBlendPoseApplied);
    else CVR_DIAGNOSTIC(++CyberpunkVR_WorldBlendPoseChanged);
    // The thunk now calls the original vtable+240 notification with unchanged
    // registers. The engine rebuilds matrices/dirty state from these bytes.
}

namespace {
bool Matches(uintptr_t exe,const cvr::camera::sites::Site& site) {
    const auto* bytes=reinterpret_cast<const uint8_t*>(exe+site.rva);
    for(size_t i=0;site.bytes[i*2];++i) {
        unsigned expected{};
        if(std::sscanf(site.bytes+i*2,"%2x",&expected)!=1 || bytes[i]!=expected)return false;
    }
    return true;
}
bool InstallWorldSpaceBlendCamera() {
    const auto exe=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    const auto& site=cvr::camera::sites::WorldBlendNotifyCall;
    auto* call=reinterpret_cast<uint8_t*>(exe+site.rva);
    if(!exe || !Matches(exe,site) || !Matches(exe,cvr::camera::sites::WorldBlendSourceRead) ||
       !Matches(exe,cvr::camera::sites::WorldBlendStore))return false;
    auto* relay=static_cast<uint8_t*>(AllocateTrampoline(call,14));
    if(!relay)return false;
    // RIP-indirect jump preserves RAX, which contains the native vtable.
    const uint8_t jump[]={0xFF,0x25,0,0,0,0};
    std::memcpy(relay,jump,sizeof(jump));
    const auto target=reinterpret_cast<uintptr_t>(&CvrWorldBlendNotifyDetour);
    std::memcpy(relay+6,&target,sizeof(target));
    const auto distance=reinterpret_cast<intptr_t>(relay)-reinterpret_cast<intptr_t>(call+6);
    if(distance<INT32_MIN || distance>INT32_MAX)return false;
    DWORD previous{};
    if(!VirtualProtect(call,6,PAGE_EXECUTE_READWRITE,&previous))return false;
    // NOP + CALL keeps the native six-byte call's return address intact.
    call[0]=0x90;call[1]=0xE8;
    const auto relative=static_cast<int32_t>(distance);
    std::memcpy(call+2,&relative,4);
    DWORD unused{};VirtualProtect(call,6,previous,&unused);
    FlushInstructionCache(GetCurrentProcess(),relay,14);
    FlushInstructionCache(GetCurrentProcess(),call,6);
    return true;
}
CVR_HOOK("WorldSpaceBlendCamera",::cvr::hooks::Stage::Boot,82,InstallWorldSpaceBlendCamera);
}
#endif
#endif
