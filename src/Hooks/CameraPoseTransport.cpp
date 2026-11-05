#include "Utils/DebugGate.hpp"
#include "Camera/PoseIdentity.hpp"
#include "Camera/ExternalViewMath.hpp"
#include "Camera/ExternalView.hpp"
#include "Camera/PlayerCameraState.hpp"
#include "Camera/ExternalViewLease.hpp"
#include "Camera/CameraState.hpp"
#include "Stereo/StereoInternal.hpp"
#include "Hooks/Hook.hpp"
#include "Hooks/CameraPoseSites.hpp"
#include "Utils/MemorySafe.hpp"
#include "Utils/WeakObjectSlot.hpp"
#include "Core/VrCoreShared.hpp"
#include <RED4ext/Scripting/Natives/entIComponent.hpp>
#include <MinHook.h>
#include <intrin.h>
#include <cstring>
#include <cstdio>
#include <new>
#include <type_traits>
#include <vector>
#include <array>
#include "Lobby/MenuLobby.hpp"

struct PoseBlendDebug {
    uint64_t calls{},labelled{},rejected{};
    uintptr_t director{};
    uint32_t count{},accepted{};
    float weights[8]{};
    uint64_t poseIds[8]{};
    uintptr_t components[8]{};
    cvr::camera::PoseFingerprint inputs[8]{},output{};
};
struct ExternalCameraDebug {
    uint64_t writes{},stampUs{},poseId{},origin{},component{};
    int32_t position[3]{};
    float rotation[4]{},head[7]{},fov{};
};
static_assert(sizeof(ExternalCameraDebug)==104);
extern "C" {
__declspec(dllexport) PoseBlendDebug CyberpunkVR_PoseBlendDebug{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PoseBlendDebugSeq{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_PoseMainReadMissing[2]{}; // blended / override setup
__declspec(dllexport) ExternalCameraDebug CyberpunkVR_ExternalCameraDebug[2]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ExternalCameraDebugSeq[2]{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_ExternalSelectionStates[4]{}; // fresh / retained / inactive / unavailable
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_ExternalNotify[3]{}; // MAIN pushes / expired or replaced / disabled or detached
}

bool EnsureCameraPoseCopyHook();

// These engine leaf functions have a narrower clobber set than the Win64 ABI.
// In particular, +682209 calls SetupDefault with a live object in R9 and uses
// R9 again immediately after the call. C++ bookkeeping must be transparent.
extern "C" {
void* CvrPoseSetupCopyOriginal{};
void* CvrPoseSetupDefaultOriginal{};
uintptr_t CvrPoseSetupCopyDetour(uintptr_t,uintptr_t);
uintptr_t CvrPoseSetupDefaultDetour(uintptr_t);
void CvrPoseSetupDefaultBefore(uintptr_t destination) {
    cvr::camera::InvalidatePoseAddress(destination);
}
void CvrPoseSetupCopyBefore(void* storage,uintptr_t source) {
    using cvr::camera::PoseReceipt;
    static_assert(sizeof(PoseReceipt)<=0x200 && alignof(PoseReceipt)<=16);
    static_assert(std::is_trivially_destructible_v<PoseReceipt>);
    new (storage) PoseReceipt(cvr::camera::CapturePoseAddress(source));
}
void CvrPoseSetupCopyAfter(cvr::camera::PoseReceipt* receipt,uintptr_t destination) {
    using cvr::camera::PoseReceipt;
    cvr::camera::TransferPoseAddress(*receipt,destination);
    receipt->~PoseReceipt();
}
}

namespace {
using namespace cvr::camera;
using CopyFn=uintptr_t(__fastcall*)(uintptr_t,uintptr_t);
using SerializeFn=void(__fastcall*)(uintptr_t,uintptr_t);
using RttFn=double(__fastcall*)(uintptr_t,uintptr_t,char);
using RenderFn=uintptr_t(__fastcall*)(uintptr_t,uintptr_t,uintptr_t);
using BlendFn=uintptr_t(__fastcall*)(uintptr_t);
using BlendFinalizeFn=void(__fastcall*)(uintptr_t);
using RttRefreshFn=void(__fastcall*)(uintptr_t);
CopyFn s_descriptor{},s_cameraClone{};
SerializeFn s_serialize{};
SerializeFn s_genericSerialize{};
RttFn s_rtt{};
RenderFn s_render{},s_mainRead{};
BlendFn s_blend{};
BlendFinalizeFn s_blendFinalize{};
RttRefreshFn s_rttRefresh{};
uintptr_t s_exe{};
thread_local PoseReceipt t_source{};
thread_local uintptr_t t_blendDirector{};
thread_local uintptr_t t_rttComponent{};
thread_local uintptr_t t_serializingComponent{};
struct ExternalHead {OpenXRHeadPose head{};uint64_t id{};};
thread_local ExternalHead t_externalHead{};
std::mutex s_blendDebugMutex;
std::mutex s_externalDebugMutex[2];

bool ExternalAllowed() {
    // The existing device-lens and BD playback paths already own those views.
    // The main menu shown in its world is head-tracked as a scene is (Lobby/MenuLobby.hpp WorldShown).
    return (g_menuModeValue==0 || cvr::lobby::WorldShown()) && OpenXRManager::Get().IsSessionRunning() && !DeviceCamActive() && !(g_bdActive.load(std::memory_order_relaxed) &&
                                  g_bdScenePoseValid.load(std::memory_order_acquire));
}
bool ExternalHeadRead(ExternalHead& sample,bool includesFpp) {
    if(includesFpp) {
        const auto component=g_camObjMain.load(std::memory_order_acquire);
        const auto source=component ? CapturePoseAddress(component+0xE0):PoseReceipt{};
        if(!source || source.payload.view!=1 || !source.payload.head.valid ||
           source.payload.head.originSerial!=OpenXRManager::Get().GetTrackingOriginSerial())return false;
        sample={source.payload.head,source.payload.poseId};return true;
    }
    // A dormant FPP component may carry an old, still valid label. Use it only
    // when the director is actually blending that component into this view.
    uint64_t sequence{};cvr::roomscale::Vec2 consumed{};
    if(!OpenXRManager::Get().AcquireCameraPoseFrame(&sample.head,&consumed,&sequence) || !sample.head.valid)return false;
    sample.id=sequence ? sequence:NextPoseIdentity();return true;
}
bool SelectionIncludesFpp(uintptr_t director) {
    // Called inside the native serializer loop, which already holds its lock.
    uintptr_t table{};uint8_t count{};
    if(!ReadU64Safe(director+0x48,&table)||!ReadU8Safe(director+0x54,&count)||!table||count>64)return false;
    const auto main=g_camObjMain.load(std::memory_order_acquire);
    for(uint32_t i=0;i<count;++i) {
        uintptr_t camera{};float weight{};
        if(ReadU64Safe(table+i*0x20,&camera)&&camera==main&&ReadFloatSafe(table+i*0x20+0x10,&weight)&&weight>0)return true;
    }
    return false;
}
bool ReadExternalPose(uintptr_t address,ExternalViewPose& pose) {
    for(int i=0;i<3;++i){uint32_t value{};if(!ReadU32Safe(address+i*4,&value))return false;pose.position[i]=int32_t(value);}
    return ReadFloatArraySafe(reinterpret_cast<const float*>(address+16),&pose.rotation.x,4) && NormalizeExternal(pose.rotation);
}
bool WriteExternalPose(uintptr_t address,const ExternalViewPose& pose) {
    for(int i=0;i<3;++i)if(!WriteU32Safe(address+i*4,uint32_t(pose.position[i])))return false;
    for(int i=0;i<4;++i)if(!WriteFloatSafe(address+16+i*4,(&pose.rotation.x)[i]))return false;
    return true;
}
void RecordExternal(uint32_t eye,uintptr_t address,uintptr_t component,const ExternalHead& sample) {
    ExternalViewPose pose{};float fov{};
    if(!ReadExternalPose(address,pose)||!ReadFloatSafe(address+0x20,&fov))return;
    std::lock_guard lock(s_externalDebugMutex[eye]);
    auto& out=CyberpunkVR_ExternalCameraDebug[eye];
    CyberpunkVR_ExternalCameraDebugSeq[eye].fetch_add(1,std::memory_order_acq_rel);
    ++out.writes;out.stampUs=XrDiagNowUs();out.poseId=sample.id;out.origin=sample.head.originSerial;out.component=component;
    std::memcpy(out.position,pose.position.data(),12);std::memcpy(out.rotation,&pose.rotation,16);
    const auto& h=sample.head;
    const float head[]{h.posX,h.posY,h.posZ,h.oriX,h.oriY,h.oriZ,h.oriW};
    std::memcpy(out.head,head,sizeof(head));out.fov=fov;
    CyberpunkVR_ExternalCameraDebugSeq[eye].fetch_add(1,std::memory_order_release);
}
bool ComposeExternalMain(const ExternalViewPose& base,const ExternalHead& head,ExternalViewPose& output) {
    const auto& h=head.head;
    const XrPosef pose{{h.oriX,h.oriY,h.oriZ,h.oriW},{h.posX,h.posY,h.posZ}};
    const float eye=CyberpunkVR_IpdInWorldPos ? GetDesiredHalfIpd()*(CyberpunkVR_MainIsRightEye ? 1.f:-1.f):0;
    return ComposeExternal(base,pose,GetWorldScale(),eye,output);
}
struct ExternalSelection {ExternalViewPose main{};ExternalHead sample{};float fov{};bool externalView{};};
thread_local const ExternalSelection* t_externalRtt{};
std::mutex s_externalMainMutex;
ExternalViewLease<ExternalSelection> s_externalMain;
ExternalViewLease<ExternalSelection> s_aimMain;
cvr::WeakObjectSlot<RED4ext::WeakHandle<RED4ext::ISerializable>> s_externalVrcam;
void ObserveExternalVrcam(uintptr_t component) {
    // Only called with the live argument of the engine's RTT callbacks.
    if(!component)return;
    const auto* object=reinterpret_cast<RED4ext::ent::IComponent*>(component);
    if(object->ref.instance==object && object->ref.refCount)
        s_externalVrcam.Bind(component,reinterpret_cast<uintptr_t>(object->ref.refCount),object->ref);
}
ExternalViewKey ExternalKey() {
    return {g_camObjMain.load(std::memory_order_acquire),
        cvr::detail::g_vrcam_comp.load(std::memory_order_acquire),OpenXRManager::Get().GetTrackingOriginSerial()};
}
bool SceneMainHandoff() {
    // This existing scene/editor route already positions VRCAM from MAIN.
    // Carry the completed MAIN receipt with that transform as well, instead
    // of losing it at the legacy component rewrite (and disabling framegen).
    return CyberpunkVR_BdEditorAlign && g_bdActive.load(std::memory_order_relaxed) &&
        !g_bdScenePoseValid.load(std::memory_order_acquire);
}
void NotifyExternalVrcam(uintptr_t component) {
    static thread_local bool pushing=false;
    uintptr_t table{},notify{};
    if(pushing || !component || !ReadU64Safe(component,&table) || !table || !ReadU64Safe(table+0x240,&notify) || !notify)return;
    pushing=true;
    reinterpret_cast<void(__fastcall*)(uintptr_t,uintptr_t)>(notify)(component,component+0x100);
    pushing=false;
}
void PublishExternalMain(uintptr_t address,const PoseReceipt& receipt) {
    if(!receipt)return; // missing observation does not mean an FPP transition
    const auto key=ExternalKey();
    if(!ExternalAllowed()) {
        std::lock_guard lock(s_externalMainMutex);s_externalMain.Clear();s_aimMain.Clear();return;
    }
    ExternalSelection frame{};
    if(receipt.payload.head.originSerial!=key.origin || !ReadExternalPose(address,frame.main) ||
       !ReadFloatSafe(address+0x20,&frame.fov))return;
    frame.sample={receipt.payload.head,receipt.payload.poseId};
    frame.externalView=receipt.payload.externalView;
    const bool mirror=frame.externalView || SceneMainHandoff();
    {
        std::lock_guard lock(s_externalMainMutex);
        const auto now=XrDiagNowUs();
        s_aimMain.Publish(frame,key,now);
        if(mirror)s_externalMain.Publish(frame,key,now);else s_externalMain.Clear();
    }
    if(!mirror)return;
    // Refresh from this completed MAIN, without one-frame lag. The address is
    // only an identity comparison: locking the engine weak reference is what
    // proves lifetime, including if the old component has already been freed.
    const auto live=s_externalVrcam.Lock(key.eyeCamera);
    if(!live){CVR_DIAGNOSTIC(++CyberpunkVR_ExternalNotify[1]);return;}
    const auto* component=static_cast<RED4ext::ent::IComponent*>(live.GetPtr());
    if(!component->isEnabled || !component->owner ||
       key.eyeCamera!=cvr::detail::g_vrcam_comp.load(std::memory_order_acquire)) {
        CVR_DIAGNOSTIC(++CyberpunkVR_ExternalNotify[2]);return;
    }
    NotifyExternalVrcam(key.eyeCamera); // live holds the component through the call
    CVR_DIAGNOSTIC(++CyberpunkVR_ExternalNotify[0]);
}
bool ReadExternalMain(ExternalSelection& output) {
    const auto key=ExternalKey();
    std::lock_guard lock(s_externalMainMutex);
    if(!ExternalAllowed() || OpenXRManager::Get().ExternalPoseResetPending()) {
        s_externalMain.Clear();CVR_DIAGNOSTIC(++CyberpunkVR_ExternalSelectionStates[2]);return false;
    }
    const auto now=XrDiagNowUs();
    if(s_externalMain.Read(key,now,output)) {
        if(!output.externalView && !SceneMainHandoff()) {s_externalMain.Clear();return false;}
        CVR_DIAGNOSTIC(++CyberpunkVR_ExternalSelectionStates[now-s_externalMain.stamp>50000?1:0]);return true;
    }
    CVR_DIAGNOSTIC(++CyberpunkVR_ExternalSelectionStates[3]);return false;
}

uintptr_t __fastcall Blend(uintptr_t director) {
    const auto previous=t_blendDirector;
    const auto previousHead=t_externalHead;t_externalHead={};
    t_blendDirector=director;
    const auto result=s_blend(director);
    t_blendDirector=previous;
    t_externalHead=previousHead;
    return result;
}
void __fastcall BlendFinalize(uintptr_t array) {
    // The native blend is finished, but its serialized inputs still exist.
    // This call site is under our Blend scope; unrelated array frees never
    // acquire a camera label. Read the actual entries, not a table sampled
    // before the director took its own lock.
    if(t_blendDirector && reinterpret_cast<uintptr_t>(_ReturnAddress())==s_exe+0x5FBE2B) {
        uintptr_t entries{};uint32_t count{};
        const bool diagnostics=cvr::RuntimeDiagnosticsEnabled();
        PoseBlendDebug diagnostic{};
        diagnostic.director=t_blendDirector;
        const auto output=t_blendDirector+0x500; // 2.31: +0x4C0
        bool accepted=false;
        if(ReadU64Safe(array,&entries) && ReadU32Safe(array+12,&count) && count>0 && count<=64) {
            std::vector<WeightedPoseReceipt> inputs;
            inputs.reserve(count);
            bool readable=true;
            for(uint32_t i=0;i<count;++i) {
                const auto entry=entries+uintptr_t(i)*0xA0;
                float weight{};
                if(!ReadFloatSafe(entry+0x90,&weight)) { readable=false;break; }
                const auto receipt=CapturePoseAddress(entry);
                inputs.push_back({receipt,weight});
                if(diagnostics && i<8) {
                    diagnostic.weights[i]=weight;
                    diagnostic.poseIds[i]=receipt.payload.poseId;
                    diagnostic.components[i]=receipt.payload.component;
                    if(receipt)diagnostic.inputs[i]=receipt.fingerprint;
                }
            }
            if(readable)accepted=PublishBlendedPose(output,inputs);
            else InvalidatePoseAddress(output);
        } else InvalidatePoseAddress(output);
        if(accepted)PublishExternalMain(output,CapturePoseAddress(output));
        if(diagnostics) {
        diagnostic.count=count;
        diagnostic.accepted=accepted ? 1u : 0u;
        if(const auto receipt=CapturePoseAddress(output))diagnostic.output=receipt.fingerprint;
        std::lock_guard lock(s_blendDebugMutex);
        diagnostic.calls=CyberpunkVR_PoseBlendDebug.calls+1;
        diagnostic.labelled=CyberpunkVR_PoseBlendDebug.labelled+(accepted ? 1 : 0);
        diagnostic.rejected=CyberpunkVR_PoseBlendDebug.rejected+(accepted ? 0 : 1);
        CyberpunkVR_PoseBlendDebugSeq.fetch_add(1,std::memory_order_acq_rel);
        CyberpunkVR_PoseBlendDebug=diagnostic;
        CyberpunkVR_PoseBlendDebugSeq.fetch_add(1,std::memory_order_release);
        }
    }
    s_blendFinalize(array);
}

struct SourceScope {
    PoseReceipt previous;
    explicit SourceScope(const PoseReceipt& value):previous(t_source) { t_source=value; }
    ~SourceScope() { t_source=previous; }
};
bool DescriptorMatches(uintptr_t descriptor,const PoseReceipt* receipt) {
    if(!receipt || !*receipt || descriptor<0x10000)return false;
    __try {
        return std::memcmp(reinterpret_cast<const void*>(descriptor),receipt->fingerprint.rotation.data(),16)==0 &&
               std::memcmp(reinterpret_cast<const void*>(descriptor+16),receipt->fingerprint.position.data(),12)==0;
    } __except(EXCEPTION_EXECUTE_HANDLER) { return false; }
}
void __fastcall Serialize(uintptr_t interfacePointer,uintptr_t destination) {
    const auto source=CapturePoseAddress(interfacePointer-0x120+0xE0);
    InvalidatePoseAddress(destination);
    const auto previous=t_serializingComponent;
    t_serializingComponent=interfacePointer-0x120;
    s_serialize(interfacePointer,destination); // includes the existing LocateCamera hook
    t_serializingComponent=previous;
    // Locate owns takeover transforms and publishes their actual written pose.
    // The destination was invalidated before this call, so only a new, still
    // matching publication can survive here. Ordinary serialization inherits
    // the component receipt as before.
    if(!CapturePoseAddress(destination))PublishSerializedPose(destination,source);
}
void __fastcall GenericSerialize(uintptr_t interfacePointer,uintptr_t destination) {
    InvalidatePoseAddress(destination);
    s_genericSerialize(interfacePointer,destination);
    if(!t_blendDirector || !ExternalAllowed())return;
    RefreshPlayerCameraState();
    if(!t_externalHead.id && !ExternalHeadRead(t_externalHead,SelectionIncludesFpp(t_blendDirector)))return;
    ExternalViewPose base{},composed{};
    if(!ReadExternalPose(destination,base)||!ComposeExternalMain(base,t_externalHead,composed)||
       !WriteExternalPose(destination,composed))return;
    const float fov=g_normalFovOverrideValue;
    if(std::isfinite(fov)&&fov>1&&fov<179)WriteFloatSafe(destination+0x20,fov);
    PublishCameraSetupPose(destination,interfacePointer-0x120,1,t_externalHead.id,t_externalHead.head,true);
    CVR_DIAGNOSTIC(RecordExternal(0,destination,interfacePointer-0x120,t_externalHead));
}
uintptr_t __fastcall MainRead(uintptr_t director,uintptr_t destination,uintptr_t setup) {
    // Bind the actual selected setup argument. Reading director+500 here would
    // mislabel an override chosen by the director after an earlier observation.
    SourceScope scope(CapturePoseAddress(setup));
    if(!t_source)CVR_DIAGNOSTIC(CyberpunkVR_PoseMainReadMissing[setup==director+0x500 ? 0 : 1].fetch_add(1,std::memory_order_relaxed));
    InvalidatePoseAddress(destination);
    const auto result=s_mainRead(director,destination,setup);
    TransferPoseAddress(t_source,destination);
    PublishExternalMain(destination,CapturePoseAddress(destination));
    return result;
}
double __fastcall RttBuild(uintptr_t component,uintptr_t destination,char force) {
    // D29AA8 reads this component's +E0/+F0 before publishing its named view.
    // Capture BEFORE the read, including when the resulting coordinates repeat.
    SourceScope scope(CapturePoseAddress(component+0xE0));
    if(component && component==cvr::detail::g_vrcam_comp.load(std::memory_order_acquire))ObserveExternalVrcam(component);
    ExternalSelection selection{};
    const bool external=component==cvr::detail::g_vrcam_comp.load(std::memory_order_acquire) && ReadExternalMain(selection);
    const auto previousSelection=t_externalRtt;t_externalRtt=external?&selection:nullptr;
    const auto previous=t_rttComponent;t_rttComponent=component;
    const auto result=s_rtt(component,destination,external?1:force);
    t_rttComponent=previous;t_externalRtt=previousSelection;return result;
}
void __fastcall RttRefresh(uintptr_t component) {
    if(component && component==cvr::detail::g_vrcam_comp.load(std::memory_order_acquire)) {
        ObserveExternalVrcam(component);
        ExternalSelection selection{};
        if(ReadExternalMain(selection))NotifyExternalVrcam(component);
    }
    s_rttRefresh(component);
}
uintptr_t __fastcall DescriptorBuild(uintptr_t destination,uintptr_t descriptor) {
    if(t_rttComponent && t_rttComponent==cvr::detail::g_vrcam_comp.load(std::memory_order_acquire) &&
       reinterpret_cast<uintptr_t>(_ReturnAddress())==s_exe+0xD29C8C) {
        if(t_externalRtt) {
            const auto& source=*t_externalRtt;
            auto opposite=source.main;
            const float offset=CyberpunkVR_IpdInWorldPos ? GetDesiredHalfIpd()*2*(CyberpunkVR_MainIsRightEye?-1.f:1.f):0;
            if(ExternalEye(opposite,offset)) {
                // This is the native builder's temporary descriptor, not the
                // player's placed component. Matrices/culling are constructed
                // by the original from these fields; no late camera rewrite.
                // 2.5 reads the input up to +98 (the handle at +88, the sub-view offsets at +60/+70,
                // +80/+84); 2.31 stopped at +60. The caller's descriptor is a frame local (rbp+90,
                // the destination at rbp+840), so the wider read stays inside its frame.
                alignas(16) std::array<std::byte,0xA0> copy{};
                std::memcpy(copy.data(),reinterpret_cast<const void*>(descriptor),copy.size());
                std::memcpy(copy.data(),&opposite.rotation,16);
                std::memcpy(copy.data()+0x10,opposite.position.data(),12);
                const float fov=source.fov;
                if(std::isfinite(fov)&&fov>1&&fov<179)std::memcpy(copy.data()+0x38,&fov,4);
                InvalidatePoseAddress(destination);
                const auto result=s_descriptor(destination,reinterpret_cast<uintptr_t>(copy.data()));
                PublishCameraSetupPose(destination,t_rttComponent,2,source.sample.id,source.sample.head,source.externalView);
                CVR_DIAGNOSTIC(RecordExternal(1,destination,t_rttComponent,source.sample));
                return result;
            }
        }
    }
    const auto source=DescriptorMatches(descriptor,&t_source) ? t_source : PoseReceipt{};
    InvalidatePoseAddress(destination);
    const auto result=s_descriptor(destination,descriptor);
    TransferPoseAddress(source,destination);return result;
}
uintptr_t __fastcall CameraClone(uintptr_t destination,uintptr_t source) {
    const auto receipt=CapturePoseAddress(source);
    const auto result=s_cameraClone(destination,source);
    TransferPoseAddress(receipt,destination);return result;
}
uintptr_t __fastcall RenderBuild(uintptr_t destination,uintptr_t descriptor,uintptr_t clipPlanes) {
    // The two verified calls (4248F6, 424951) pass context+2ED0. Their descriptor is
    // made from context+2C50, populated by the registered-camera copy. 2.31 had one
    // call (4E50F9) with context+20A0 / +1E20: the context block moved up by 0xE30.
    PoseReceipt source{};
    const auto returnAddress=reinterpret_cast<uintptr_t>(_ReturnAddress());
    if((returnAddress==s_exe+0x4248FB || returnAddress==s_exe+0x424956) && clipPlanes>0x2ED0) {
        const auto context=clipPlanes-0x2ED0;
        source=CapturePoseAddress(context+0x2C50);
        if(!DescriptorMatches(descriptor,&source))source={};
    }
    InvalidatePoseAddress(destination);
    const auto result=s_render(destination,descriptor,clipPlanes);
    TransferPoseAddress(source,destination);return result;
}

bool Matches(const cvr::camera::sites::Site& site) {
    const auto* bytes=reinterpret_cast<const uint8_t*>(s_exe+site.rva);
    for(size_t i=0;site.bytes[i*2];++i) {
        unsigned expected{};
        if(std::sscanf(site.bytes+i*2,"%2x",&expected)!=1 || bytes[i]!=expected)return false;
    }
    return true;
}
struct Site { cvr::camera::sites::Site expected;void* hook;void** original; };
bool InstallCameraPoseTransport() {
    s_exe=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    if(!s_exe)return false;
    const Site sites[]={
        {cvr::camera::sites::Blend,reinterpret_cast<void*>(&Blend),reinterpret_cast<void**>(&s_blend)},
        {cvr::camera::sites::BlendFinalize,reinterpret_cast<void*>(&BlendFinalize),reinterpret_cast<void**>(&s_blendFinalize)},
        {cvr::camera::sites::Serialize,reinterpret_cast<void*>(&Serialize),reinterpret_cast<void**>(&s_serialize)},
        {cvr::camera::sites::GenericSerialize,reinterpret_cast<void*>(&GenericSerialize),reinterpret_cast<void**>(&s_genericSerialize)},
        {cvr::camera::sites::SetupCopy,reinterpret_cast<void*>(&CvrPoseSetupCopyDetour),&CvrPoseSetupCopyOriginal},
        {cvr::camera::sites::SetupDefault,reinterpret_cast<void*>(&CvrPoseSetupDefaultDetour),&CvrPoseSetupDefaultOriginal},
        {cvr::camera::sites::MainRead,reinterpret_cast<void*>(&MainRead),reinterpret_cast<void**>(&s_mainRead)},
        {cvr::camera::sites::RttBuild,reinterpret_cast<void*>(&RttBuild),reinterpret_cast<void**>(&s_rtt)},
        {cvr::camera::sites::RttRefresh,reinterpret_cast<void*>(&RttRefresh),reinterpret_cast<void**>(&s_rttRefresh)},
        {cvr::camera::sites::DescriptorBuild,reinterpret_cast<void*>(&DescriptorBuild),reinterpret_cast<void**>(&s_descriptor)},
        {cvr::camera::sites::CameraClone,reinterpret_cast<void*>(&CameraClone),reinterpret_cast<void**>(&s_cameraClone)},
        {cvr::camera::sites::RenderBuild,reinterpret_cast<void*>(&RenderBuild),reinterpret_cast<void**>(&s_render)}
    };
    for(const auto& site:sites)if(!Matches(site.expected))return false;
    if(!Matches(cvr::camera::sites::RenderCall) || !Matches(cvr::camera::sites::BlendFinalizeCall) ||
       !Matches(cvr::camera::sites::RttDescriptorCall)||
       !EnsureCameraPoseCopyHook())return false;
    bool ok=true;
    for(const auto& site:sites) {
        void* target=reinterpret_cast<void*>(s_exe+site.expected.rva);
        const auto created=MH_CreateHook(target,site.hook,site.original);
        const auto enabled=created==MH_OK ? MH_EnableHook(target) : created;
        if(created!=MH_OK || enabled!=MH_OK) {
            Log("PoseIdentity: hook +%llX failed create=%d enable=%d\n",site.expected.rva,int(created),int(enabled));
            ok=false;
        }
    }
    return ok;
}
CVR_HOOK("CameraPoseTransport",::cvr::hooks::Stage::Boot,81,InstallCameraPoseTransport);
}

uintptr_t cvr::camera::CurrentSerializedCameraComponent() {return t_serializingComponent;}

bool cvr::camera::ReadMainAimPose(MainAimPose& out) {
    if(!ExternalAllowed() || OpenXRManager::Get().ExternalPoseResetPending())return false;
    ExternalSelection frame{};const auto key=ExternalKey();const auto now=XrDiagNowUs();
    {
        std::lock_guard lock(s_externalMainMutex);
        if(!s_aimMain.Read(key,now,frame) || now-s_aimMain.stamp>100000)return false;
    }
    const float offset=CyberpunkVR_IpdInWorldPos ? GetDesiredHalfIpd()*(CyberpunkVR_MainIsRightEye?-1.f:1.f):0;
    if(!ExternalEye(frame.main,offset))return false;
    MainAimPose value{};
    for(int i=0;i<3;++i)value.position[i]=float(frame.main.position[i])/131072.f;
    std::memcpy(value.rotation,&frame.main.rotation,sizeof(value.rotation));
    value.poseId=frame.sample.id;out=value;return true;
}

bool cvr::camera::ReadExternalMainDirection(float out[3],uint64_t* poseId) {
    if(!out || !ExternalAllowed() || OpenXRManager::Get().ExternalPoseResetPending())return false;
    ExternalSelection frame{};const auto key=ExternalKey();const auto now=XrDiagNowUs();
    {
        std::lock_guard lock(s_externalMainMutex);
        // Steering needs a recent observation even though a missing MAIN update
        // is never a reason to move the rendered second eye back into the cabin.
        if(!s_externalMain.Read(key,now,frame)||!frame.externalView||now-s_externalMain.stamp>100000)return false;
    }
    XrVector3f forward{};if(!PlanarExternalForward(frame.main.rotation,forward))return false;
    out[0]=forward.x;out[1]=forward.y;out[2]=0;
    if(poseId)*poseId=frame.sample.id;return true;
}
