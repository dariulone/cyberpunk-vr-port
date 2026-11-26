#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
// The body's heading at the native movement adapter. THE HOOK IS RUST'S with CVR_RUST_VRIK
// (rust/crates/cvr-rig/src/vrik/body_heading.rs): this file then keeps only the registration (so the install
// order holds), and Hooks/BodyHeading.hpp forwards from src/Hooks/RoomscaleRust.cpp.
#include "Hooks/BodyHeading.hpp"
#include "Hooks/Hook.hpp"
#include "Hooks/RoomscaleMove.hpp"
#include "Hooks/RoomscaleSites.hpp"
#include "Camera/CameraState.hpp"
#include "Runtimes/NativeBodyHeading.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include <MinHook.h>
#include <windows.h>
#include <cmath>
#include <cstring>
#include <mutex>
#include <cstdio>

#if !CVR_RUST_VRIK
extern void Log(const char* fmt, ...);
extern "C" float BodyYawFollowStep(float observedOffset);
#endif

// The exports are src/Hooks/BodyHooksState.cpp's (both builds).
extern "C" {
extern int CyberpunkVR_BodyHeadingHooksReady,CyberpunkVR_DebugBodyHeadingSkip;
extern uint64_t CyberpunkVR_BodyHeadingCommits,CyberpunkVR_BodyHeadingRequests;
#if !CVR_RUST_VRIK
extern float CyberpunkVR_BodyTrackingYawRad;
#endif
}

#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK   // registered from rust/crates/cvr-render/src/hook_table.rs
CVR_HOOK("BodyHeading",::cvr::hooks::Stage::Boot,79,cvr_hook_body_heading_install);
#endif
#else
extern "C" uint64_t cvr_cpp_xr_tracking_origin_serial();   // src/Camera/CameraServices.cpp
// The motion request, the adapter's apply, and the binding of the originals: C linkage, so tools/vrik_tests calls the
// same names in both builds.
extern "C" void cvr_body_heading_motion(void* provider,float* position,float* rotation);
extern "C" bool cvr_body_heading_apply(void* adapter,const float* position,const float* rotation,uint8_t flag);

namespace {
std::mutex s_mutex;
cvr::body::NativeBodyHeading s_heading;
uintptr_t s_base{};
using ApplyFn = bool(__fastcall*)(void*, const float*, const float*, uint8_t);
ApplyFn s_apply{};
using MotionFn = void(__fastcall*)(void*, float*, float*);
MotionFn s_motion{};
struct PendingTurn {
    uintptr_t adapter{},owner{};
    uint64_t origin{};
    float nativeTurn{},rotation[4]{};
    bool valid{};
};
thread_local PendingTurn s_pending;


template<class T> T Read(const void* p, size_t offset) {
    T value; std::memcpy(&value, static_cast<const uint8_t*>(p) + offset, sizeof(value)); return value;
}
bool PlayerAdapter(void* adapter) {
    __try {
        auto* state = reinterpret_cast<const void*>(Read<uintptr_t>(adapter, 0x18));
        return cvr::roomscale::IsPlayerMovement(state) &&
            Read<uintptr_t>(state, 0x160) == reinterpret_cast<uintptr_t>(adapter) &&
            cvr::roomscale::IsPlayerProvider(reinterpret_cast<const void*>(Read<uintptr_t>(state, 0xA8)));
    } __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
}
bool Yaw(const float* q, float& yaw) {
    const float n=q[0]*q[0]+q[1]*q[1]+q[2]*q[2]+q[3]*q[3];
    if(!std::isfinite(n) || n<.5f || n>1.5f) return false;
    yaw=std::atan2(2*(q[3]*q[2]+q[0]*q[1]),1-2*(q[1]*q[1]+q[2]*q[2]));
    return true;
}
}

// PhysicalMoveController::GetDeltaTransform, vtable+20. This is a motion
// request upstream of ApplyPostCharacterControllerTransforms, not a transform
// write. The camera-input fields and the provider's velocity getter are intact.
void cvr_body_heading_motion(void* provider,float* position,float* rotation) {
    s_motion(provider,position,rotation);
    s_pending={};
    if(!CyberpunkVR_BodyHeadingHooksReady || !cvr::roomscale::IsPlayerProvider(provider) ||
       !cvr::roomscale::PhysicalBodyHeadingOwned())return;
    auto* state=reinterpret_cast<const void*>(Read<uintptr_t>(provider,0x10));
    auto* adapter=reinterpret_cast<void*>(Read<uintptr_t>(state,0x160));
    if(!adapter || Read<uintptr_t>(adapter,0)!=s_base+cvr::roomscale::sites::PhysicalAdapterVtable ||
       !PlayerAdapter(adapter))return;
    const auto owner=cvr::roomscale::PlayerIdentity();
    const auto origin=cvr_cpp_xr_tracking_origin_serial();
    float before{},nativeTurn{};
    if(!Yaw(reinterpret_cast<const float*>(static_cast<const uint8_t*>(adapter)+0x30),before) ||
       !Yaw(rotation,nativeTurn))return;
    float offset{};
    {
        std::lock_guard lock(s_mutex);
        s_heading.Observe(owner,origin,before);
        offset=s_heading.Offset();
    }
    const float step=CyberpunkVR_DebugBodyHeadingSkip ? 0 : BodyYawFollowStep(offset);
    const float s=std::sin(step*.5f),c=std::cos(step*.5f);
    const float delta[4]={c*rotation[0]-s*rotation[1],c*rotation[1]+s*rotation[0],
                          c*rotation[2]+s*rotation[3],c*rotation[3]-s*rotation[2]};
    if(step!=0)std::memcpy(rotation,delta,sizeof(delta));
    s_pending.adapter=reinterpret_cast<uintptr_t>(adapter);
    s_pending.owner=owner;s_pending.origin=origin;s_pending.nativeTurn=nativeTurn;
    std::memcpy(s_pending.rotation,rotation,sizeof(s_pending.rotation));
    s_pending.valid=true;
    ++CyberpunkVR_BodyHeadingRequests;
}

bool cvr_body_heading_apply(void* adapter,const float* position,const float* rotation,uint8_t flag) {
    // This observer never alters a native request or its destination. Only a
    // matching, actually accepted request commits the independent view reference.
    const auto pending=s_pending;
    const bool matching=pending.valid && pending.adapter==reinterpret_cast<uintptr_t>(adapter) &&
        pending.owner==cvr::roomscale::PlayerIdentity() &&
        std::memcmp(pending.rotation,rotation,sizeof(pending.rotation))==0;
    if(matching)s_pending.valid=false;
    const bool applied=s_apply(adapter,position,rotation,flag);
    float after{};
    if(matching && applied && Yaw(reinterpret_cast<const float*>(static_cast<const uint8_t*>(adapter)+0x30),after)) {
        std::lock_guard lock(s_mutex);
        if(s_heading.Matches(pending.owner,pending.origin)) {
            s_heading.Commit(after,pending.nativeTurn);
            CyberpunkVR_BodyTrackingYawRad=s_heading.View();
            CyberpunkVR_BodyYawRealignRad=s_heading.Offset();
            CyberpunkVR_DebugBodyFollowOffsetDeg=s_heading.Offset()*57.2957795f;
            ++CyberpunkVR_BodyHeadingCommits;
        }
    }
    return applied;
}

// The image base and the two originals: the install's, or the tests' fakes.
extern "C" void cvr_body_heading_bind(uintptr_t base,void* apply,void* motion) {
    s_base=base;s_apply=reinterpret_cast<ApplyFn>(apply);s_motion=reinterpret_cast<MotionFn>(motion);
}

namespace {
bool Matches(uintptr_t rva,const char* hex) {
    for(size_t i=0;hex[2*i];++i) {
        unsigned byte{};
        if(std::sscanf(hex+2*i,"%2x",&byte)!=1 || *reinterpret_cast<const uint8_t*>(s_base+rva+i)!=byte)return false;
    }
    return true;
}
bool Install() {
    s_base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    if(!s_base || !Matches(0xA99838,"48895C240848897C241055488BEC4883EC70") ||
       !Matches(0xE3C3A0,"40534883EC300F2805F3F39702498BD80F1102F30F1089A4000000")) {
        Log("BodyHeading: native ABI mismatch; hooks not installed.\n");return false;
    }
    const auto init=MH_Initialize();
    if(init!=MH_OK && init!=MH_ERROR_ALREADY_INITIALIZED)return false;
    void* apply=reinterpret_cast<void*>(s_base+0xA99838);
    void* motion=reinterpret_cast<void*>(s_base+0xE3C3A0);
    if(MH_CreateHook(apply,reinterpret_cast<void*>(&cvr_body_heading_apply),reinterpret_cast<void**>(&s_apply))!=MH_OK)return false;
    if(MH_CreateHook(motion,reinterpret_cast<void*>(&cvr_body_heading_motion),reinterpret_cast<void**>(&s_motion))!=MH_OK) {
        MH_RemoveHook(apply);return false;
    }
    if(MH_EnableHook(apply)!=MH_OK || MH_EnableHook(motion)!=MH_OK) {
        MH_DisableHook(apply);MH_DisableHook(motion);MH_RemoveHook(apply);MH_RemoveHook(motion);return false;
    }
    CyberpunkVR_BodyHeadingHooksReady=1;
    Log("BodyHeading: native body motion source and commit observer installed (2.31).\n");return true;
}
}

bool BodyHeadingReady() { return CyberpunkVR_BodyHeadingHooksReady!=0; }
float BodyCentreYaw(float fallback) {
    if(!cvr::roomscale::PhysicalBodyHeadingOwned())return fallback;
    const auto owner=cvr::roomscale::PlayerIdentity();
    std::lock_guard lock(s_mutex);
    return s_heading.OwnedBy(owner) ? s_heading.CentreYaw():fallback;
}
float BodyTrackingYaw(float fallback) {
    float yaw{};
    return ReadBodyTrackingYaw(&yaw) ? yaw : fallback;
}
bool ReadBodyTrackingYaw(float* yaw) {
    if(!yaw)return false;
    if(!cvr::roomscale::PhysicalBodyHeadingOwned()) {
        BodyHeadingReset();return false;
    }
    const auto owner=cvr::roomscale::PlayerIdentity();
    const auto origin=cvr_cpp_xr_tracking_origin_serial();
    std::lock_guard lock(s_mutex);
    if(!s_heading.Matches(owner,origin))return false;
    *yaw=s_heading.View();return true;
}
float BodyHeadingOffset() {
    if(!cvr::roomscale::PhysicalBodyHeadingOwned())return 0;
    const auto owner=cvr::roomscale::PlayerIdentity();
    const auto origin=cvr_cpp_xr_tracking_origin_serial();
    std::lock_guard lock(s_mutex);
    return s_heading.Matches(owner,origin) ? s_heading.Offset() : 0;
}
void BodyHeadingReset() {
    std::lock_guard lock(s_mutex);s_heading.Reset();
    CyberpunkVR_BodyTrackingYawRad=0;
    CyberpunkVR_BodyYawRealignRad=0;
    CyberpunkVR_DebugBodyFollowOffsetDeg=0;
}
CVR_HOOK("BodyHeading",::cvr::hooks::Stage::Boot,79,Install);
#endif
#endif
