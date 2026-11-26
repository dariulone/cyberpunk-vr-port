#include "Camera/SurveillanceFollow.hpp"
#include "Camera/CameraState.hpp"
#include "Core/VrCoreShared.hpp"
#include "Hooks/Hook.hpp"
#include "Hooks/Trampoline.hpp"
#include "Overlay/VrOverlay.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include "Utils/DebugGate.hpp"
#include "Utils/MemorySafe.hpp"
#include "Anim/CharacterRig.hpp"   // VRIK_ClockMs: the clock the tests can set
#include <RED4ext/Scripting/Natives/entEntity.hpp>
#include <RED4ext/Scripting/Natives/entIComponent.hpp>
#include <RED4ext/RTTITypes.hpp>
#include <cstring>
#include <limits>
#include <mutex>

extern "C" {
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's is rust/crates/cvr-render/src/asm_thunks/surveillance_input_thunk.rs'
void* CvrSurveillanceInputOriginal{};
#endif
#if !CVR_RUST_VRIK
void CvrSurveillanceInputDetour();
void CvrSurveillanceFinishDetour();
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/render_moved_exports.rs.
#else
__declspec(dllexport) int CyberpunkVR_SurveillanceBodyFollow=1;
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_SurveillanceBodyUpdates{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_SurveillanceBodyInjected{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_SurveillanceBodyDebugSeq{};
__declspec(dllexport) float CyberpunkVR_SurveillanceBodyDebug[9]{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_SurveillanceAutoFiltered{};
__declspec(dllexport) float CyberpunkVR_SurveillanceAutoDebug[6]{};
#endif
}
// What the engine's RTTI says about the device camera control component (src/Hooks/HookServices.cpp): its owner, the
// owner's entity, whether the component is enabled, and the owner's kind (1 SurveillanceCamera, 2 SniperNest).
struct SurveillanceControl {uintptr_t owner{};uint64_t entity{};bool enabled{};uint8_t kind{};};
#if !CVR_RUST_VRIK
extern "C" bool cvr_cpp_surveillance_control(uintptr_t component,SurveillanceControl* out);
#endif

#if CVR_RUST_VRIK
// rust/crates/cvr-rig/src/vrik/surveillance_body.rs: the input and finish callbacks the thunks call, the view yaw, the
// sniper's head follow and the two patches.
extern "C" {
bool cvr_surveillance_view_yaw(uintptr_t lens,uintptr_t owner,uint64_t origin,float ownerYaw,float* yaw);
bool cvr_sniper_head_follow_active();
}
#if !CVR_RUST_VRIK   // registered from rust/crates/cvr-render/src/hook_table.rs
CVR_HOOK("SurveillanceBody",::cvr::hooks::Stage::Boot,88,cvr_hook_surveillance_body_install);
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
bool cvr::camera::SurveillanceViewYaw(uintptr_t lens,uintptr_t owner,uint64_t origin,float ownerYaw,float& yaw) {
    return cvr_surveillance_view_yaw(lens,owner,origin,ownerYaw,&yaw);
}
bool cvr::camera::SniperHeadFollowActive() { return cvr_sniper_head_follow_active(); }
#endif
#else
// OpenXR's pose sample and the takeover through src/Hooks/HookServices.cpp, as the Rust port reads them.
extern "C" bool cvr_cpp_xr_acquire_camera_pose_frame(OpenXRHeadPose* head,cvr::roomscale::Vec2* consumed,uint64_t* sequence);
namespace {
std::mutex mutex;
cvr::camera::SurveillanceFollow follow;
uintptr_t sourceLens{},sourceOwner{},sourceControl{};
uint64_t sourceEntity{};
std::atomic<uint64_t> sniperEntity{},sniperStamp{};
thread_local cvr::camera::SurveillanceInputSample pendingInput;
bool Yaw(uintptr_t quaternion,float& yaw,float* pitch=nullptr) {
    float q[4]{};
    if(!ReadFloatArraySafe(reinterpret_cast<const float*>(quaternion),q,4)||!IsPlausibleUnitQuaternion(q))return false;
    const float x=2*(q[0]*q[1]-q[2]*q[3]),y=1-2*(q[0]*q[0]+q[2]*q[2]);
    if(x*x+y*y<1e-6f)return false;
    yaw=std::atan2(-x,y);
    if(pitch)*pitch=std::atan2(2*(q[1]*q[2]+q[0]*q[3]),std::hypot(x,y));
    return true;
}
void Clear() {std::lock_guard lock(mutex);follow.Reset();sourceLens=sourceOwner=sourceControl=0;sourceEntity=0;sniperEntity.store(0,std::memory_order_release);}
bool Install() {
    const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    // Inside DeviceCameraControlComponent's native update, RDI is the live
    // component. Its six-argument input reader fills EulerAngles (Roll/Pitch/Yaw).
    auto* site=reinterpret_cast<uint8_t*>(base+0x2B34F4E);
    const uint8_t componentRegister[]={0x48,0x8b,0xf9};
    // Verify the actual instructions immediately preceding the call separately:
    // mov r8,r14; mov rcx,r10 (4D 8B C6 / 49 8B CA).
    const uint8_t before[]={0x4d,0x8b,0xc6,0x49,0x8b,0xca};
    int32_t oldOffset{};std::memcpy(&oldOffset,site+1,4);
    if(std::memcmp(reinterpret_cast<void*>(base+0x2B34ED1),componentRegister,3)||
       std::memcmp(site-6,before,6)||site[0]!=0xe8||base+0x2B34F53+oldOffset!=base+0x648844)return false;
    auto* finish=reinterpret_cast<uint8_t*>(base+0x2B351B2);
    const uint8_t finishBytes[]={0xf3,0x0f,0x10,0x44,0x24,0x44}; // movss xmm0,[rsp+44]
    if(std::memcmp(finish,finishBytes,sizeof(finishBytes)))return false;
    struct Patch {uint8_t* site;size_t size;uintptr_t thunk;uint8_t original[6]{},bytes[6]{};DWORD protection{};};
    Patch patches[]={{site,5,reinterpret_cast<uintptr_t>(&CvrSurveillanceInputDetour)},
                     {finish,6,reinterpret_cast<uintptr_t>(&CvrSurveillanceFinishDetour)}};
    for(auto& patch:patches) {
        auto* relay=static_cast<uint8_t*>(AllocateTrampoline(patch.site,14));if(!relay)return false;
        const auto distance=reinterpret_cast<intptr_t>(relay)-reinterpret_cast<intptr_t>(patch.site+5);
        if(distance<std::numeric_limits<int32_t>::min()||distance>std::numeric_limits<int32_t>::max())return false;
        const uint8_t jump[]={0xff,0x25,0,0,0,0};std::memcpy(relay,jump,6);
        std::memcpy(relay+6,&patch.thunk,8);FlushInstructionCache(GetCurrentProcess(),relay,14);
        patch.bytes[0]=0xe8;patch.bytes[5]=0x90;
        const auto offset=static_cast<int32_t>(distance);std::memcpy(patch.bytes+1,&offset,4);
        std::memcpy(patch.original,patch.site,patch.size);
    }
    CvrSurveillanceInputOriginal=reinterpret_cast<void*>(base+0x648844);
    // Both sites share one code page. Protect/restore the span once, otherwise
    // the second site's saved protection would already be PAGE_EXECUTE_READWRITE.
    const auto span=static_cast<size_t>(finish+sizeof(finishBytes)-site);
    DWORD protection{},ignored{};
    if(!VirtualProtect(site,span,PAGE_EXECUTE_READWRITE,&protection))return false;
    for(auto& patch:patches)std::memcpy(patch.site,patch.bytes,patch.size);
    const bool restored=VirtualProtect(site,span,protection,&ignored)!=0;
    if(!restored) {
        for(auto& patch:patches)std::memcpy(patch.site,patch.original,patch.size);
        VirtualProtect(site,span,protection,&ignored);
    }
    for(const auto& patch:patches)FlushInstructionCache(GetCurrentProcess(),patch.site,patch.size);
    return restored;
}
CVR_HOOK("SurveillanceBody",::cvr::hooks::Stage::Boot,88,Install);
}

extern "C" void CvrSurveillanceInputAfter(uintptr_t component,float* delta) {
    pendingInput.Capture(0,nullptr,false);
    if(!CyberpunkVR_SurveillanceBodyFollow || !CyberpunkVR_DeviceCamOrient || !DeviceCamActive()) {Clear();return;}
    SurveillanceControl control{};
    if(!cvr_cpp_surveillance_control(component,&control))return;
    const auto owner=control.owner;
    const auto lens=g_lensComp.load(std::memory_order_acquire);
    if(!owner || owner!=g_takeoverEntity.load(std::memory_order_acquire) || !lens || !control.enabled)return;
    const bool sniper=control.kind==2;
    if(control.kind==0)return;
    uintptr_t nativeLens{};
    if(!ReadPtrSafe(component+0xE0,&nativeLens)||nativeLens!=lens)return;
    // This callback exists only on the native active-controller update path.
    // TCS IsInputLockedFromQuest is NOT a camera-rotation gate: this camera
    // continues to accept mouse rotation while that script flag is true.
    if(!delta||!std::isfinite(delta[0]+delta[1]+delta[2]))return;
    uintptr_t root{};float ownerYaw{},nativeYaw{},nativePitch{};
    if(!ReadPtrSafe(owner+0xB0,&root)||!root||!Yaw(root+0xF0,ownerYaw)||!Yaw(lens+0xF0,nativeYaw,sniper?&nativePitch:nullptr))return;
    uintptr_t feature{};float currentYaw{},minimum{},maximum{};
    if(!ReadPtrSafe(component+0xC0,&feature)||!feature||!ReadFloatSafe(feature+0x40,&currentYaw)||
       !ReadFloatSafe(component+0x110,&minimum)||!ReadFloatSafe(component+0x114,&maximum))return;
    const float manualYaw=cvr::camera::SurveillanceManualYaw(currentYaw,delta[2],minimum,maximum);
    OpenXRHeadPose head{};cvr::roomscale::Vec2 consumed{};uint64_t sequence{};
    if(!cvr_cpp_xr_acquire_camera_pose_frame(&head,&consumed,&sequence)||!head.valid)return;
    const bool enabled=g_menuModeValue==0 && !cvr::vrui::CapturesInput();
    const uint64_t entity=control.entity;
    cvr::camera::SurveillanceAngles angles{};
    std::lock_guard lock(mutex);
    if(!cvr::camera::SurveillanceHeadAngles(head.oriX,head.oriY,head.oriZ,head.oriW,follow.LastHeadYaw(),angles))return;
    constexpr float radians=cvr::camera::SurveillancePi/180.f;
    const auto added=follow.Step(component,entity,head.originSerial,VRIK_ClockMs(),angles,nativeYaw,ownerYaw,
                                 manualYaw*radians,enabled,sniper,nativePitch,delta[1]*radians);
    sourceLens=lens;sourceOwner=owner;sourceControl=component;sourceEntity=entity;
    sniperStamp.store(VRIK_ClockMs(),std::memory_order_relaxed);
    sniperEntity.store(sniper?entity:0,std::memory_order_release);
    CVR_DIAGNOSTIC(
        CyberpunkVR_SurveillanceBodyDebugSeq.fetch_add(1,std::memory_order_acq_rel);
        ++CyberpunkVR_SurveillanceBodyUpdates;
        CyberpunkVR_SurveillanceBodyDebug[0]=delta[2];CyberpunkVR_SurveillanceBodyDebug[1]=delta[1];
        CyberpunkVR_SurveillanceBodyDebug[2]=added.yaw/radians;CyberpunkVR_SurveillanceBodyDebug[3]=added.pitch/radians;
        CyberpunkVR_SurveillanceBodyDebug[4]=nativeYaw/radians;CyberpunkVR_SurveillanceBodyDebug[5]=ownerYaw/radians;
        CyberpunkVR_SurveillanceBodyDebug[6]=angles.yaw/radians;CyberpunkVR_SurveillanceBodyDebug[7]=angles.pitch/radians;
        float viewYaw{};follow.View(component,entity,head.originSerial,VRIK_ClockMs(),ownerYaw,viewYaw);
        CyberpunkVR_SurveillanceBodyDebug[8]=viewYaw/radians;
        if(added.yaw!=0 || added.pitch!=0)++CyberpunkVR_SurveillanceBodyInjected;
        CyberpunkVR_SurveillanceBodyDebugSeq.fetch_add(1,std::memory_order_release);
    );
    // The original update continues with the combined input, then applies the
    // native limits/pole guard and submits its animation feature exactly once.
    delta[1]+=added.pitch/radians;delta[2]+=added.yaw/radians;
    pendingInput.Capture(component,delta,enabled);
}

extern "C" void CvrSurveillanceFinishInput(uintptr_t component,float* automatic,float* delta) {
    float before[6]{};
    CVR_DIAGNOSTIC(std::copy_n(delta,3,before);std::copy_n(automatic,3,before+3));
    if(CyberpunkVR_SurveillanceBodyFollow && pendingInput.Finish(component,automatic,delta)) {
        CVR_DIAGNOSTIC(std::copy_n(before,6,CyberpunkVR_SurveillanceAutoDebug);++CyberpunkVR_SurveillanceAutoFiltered);
    }
}

bool cvr::camera::SurveillanceViewYaw(uintptr_t lens,uintptr_t owner,uint64_t origin,float ownerYaw,float& yaw) {
    if(!CyberpunkVR_SurveillanceBodyFollow)return false;
    std::lock_guard lock(mutex);
    if(lens!=sourceLens||owner!=sourceOwner)return false;
    // The active source is supplied by a live native callback; no stored game
    // pointer is dereferenced on the camera/render side.
    return follow.View(sourceControl,sourceEntity,origin,VRIK_ClockMs(),ownerYaw,yaw);
}

bool cvr::camera::SniperHeadFollowActive() {
    const auto entity=sniperEntity.load(std::memory_order_acquire);
    const auto stamp=sniperStamp.load(std::memory_order_relaxed),now=VRIK_ClockMs();
    return entity && entity==g_takeoverEntityId.load(std::memory_order_acquire) &&
        g_remoteCamOn.load(std::memory_order_acquire) && now>=stamp && now-stamp<300 && DeviceCamActive();
}
#endif
