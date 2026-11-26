#include "Utils/DebugGate.hpp"
#include "Camera/PoseIdentity.hpp"
#include "Camera/CameraState.hpp"
#include "Utils/MemorySafe.hpp"
#include "Render/ViewCameraLayout.hpp"
#include <atomic>
#include <cstring>
#include <vector>

// The counters are defined in src/Camera/CameraLinkState.cpp (both builds); OpenXR is asked through
// src/Camera/CameraServices.cpp. Under CVR_RUST_VRIK rust/crates/cvr-rig/src/vrik/pose_identity.rs runs instead
// (src/Camera/PoseIdentityRust.cpp).
extern "C" {
extern std::atomic<uint64_t> CyberpunkVR_PoseIdComponent[2];
extern std::atomic<uint64_t> CyberpunkVR_PoseIdCopied;
extern std::atomic<uint64_t> CyberpunkVR_PoseIdCopyChanged;
uint64_t cvr_cpp_xr_tracking_origin_serial();
bool cvr_cpp_xr_resolve_rendered_pose(const OpenXRHeadPose* head,XrPosef* out);
bool cvr_cpp_xr_external_pose_reset_pending();
}
namespace cvr::camera {
namespace {
PoseAddressLedger<PoseIdentity> s_addresses;
std::atomic<uint64_t> s_poseId{0},s_writeId{0};
thread_local PoseIdentity t_nodePose{};
bool Fingerprint(uintptr_t address,PoseFingerprint* out) {
    if(address<0x10000 || !out)return false;
    __try {
        std::memcpy(out->position.data(),reinterpret_cast<const void*>(address),12);
        std::memcpy(out->rotation.data(),reinterpret_cast<const void*>(address+16),16);
        return true;
    } __except(EXCEPTION_EXECUTE_HANDLER) { return false; }
}
}
uint64_t NextPoseIdentity() {
    // Native packets already use bit63; XR-latched samples use neither bit.
    // Keep legacy, separately located samples in their own identity domain.
    return (s_poseId.fetch_add(1,std::memory_order_relaxed)+1) | (uint64_t{1}<<62);
}
void InvalidatePoseAddress(uintptr_t address) { s_addresses.Invalidate(address); }
PoseReceipt CapturePoseAddress(uintptr_t address) {
    PoseFingerprint fingerprint{};
    return Fingerprint(address,&fingerprint) ? s_addresses.Capture(address,fingerprint) : PoseReceipt{};
}
bool TransferPoseAddress(const PoseReceipt& source,uintptr_t destination) {
    PoseFingerprint sourceNow{},destinationNow{};
    if(!source || !Fingerprint(source.source,&sourceNow) || !Fingerprint(destination,&destinationNow)) {
        s_addresses.Invalidate(destination);return false;
    }
    const bool copied=s_addresses.Transfer(source,sourceNow,destination,destinationNow);
    CVR_DIAGNOSTIC((copied ? CyberpunkVR_PoseIdCopied : CyberpunkVR_PoseIdCopyChanged).fetch_add(1,std::memory_order_relaxed));
    return copied;
}
void PublishComponentPose(uintptr_t component,uint32_t view,uint64_t poseId,const OpenXRHeadPose& head) {
    PublishCameraSetupPose(component+0xE0,component,view,poseId,head);
}
void PublishCameraSetupPose(uintptr_t address,uintptr_t component,uint32_t view,uint64_t poseId,const OpenXRHeadPose& head,bool externalView) {
    PoseIdentity value{};
    PoseFingerprint fingerprint{};
    if(!poseId || view<1 || view>2 || !Fingerprint(address,&fingerprint) ||
       !cvr_cpp_xr_resolve_rendered_pose(&head,&value.localHead)) {
        s_addresses.Invalidate(address);return;
    }
    value.poseId=poseId;value.writeId=s_writeId.fetch_add(1,std::memory_order_relaxed)+1;
    value.component=component;value.view=view;value.head=head;
    value.externalView=externalView;
    s_addresses.Publish(address,fingerprint,value);
    CVR_DIAGNOSTIC(CyberpunkVR_PoseIdComponent[view-1].fetch_add(1,std::memory_order_relaxed));
}
void PublishSerializedPose(uintptr_t destination,const PoseReceipt& source) {
    // Serialisation on the component path preserves these fields. A camera
    // override changing them must supply its own label, not inherit this one.
    PoseFingerprint output{};
    if(!source || !Fingerprint(destination,&output) || output!=source.fingerprint) {
        s_addresses.Invalidate(destination);return;
    }
    TransferPoseAddress(source,destination);
}
bool ReadCameraPoseIdentity(uintptr_t camera,uint32_t view,PoseIdentity* out) {
    if(!out || view<1 || view>2 || cvr_cpp_xr_external_pose_reset_pending())return false;
    const auto record=CapturePoseAddress(camera);
    if(!record || record.payload.view!=view ||
       record.payload.head.originSerial!=cvr_cpp_xr_tracking_origin_serial())return false;
    *out=record.payload;return true;
}
bool PublishBlendedPose(uintptr_t destination,std::span<const WeightedPoseReceipt> inputs) {
    PoseFingerprint output{};
    if(!Fingerprint(destination,&output)) { s_addresses.Invalidate(destination);return false; }
    std::vector<PoseAddressLedger<PoseIdentity>::BlendInput> verified;
    verified.reserve(inputs.size());
    for(const auto& input:inputs) {
        PoseFingerprint current{};
        if(input.weight!=0 && (!input.source || input.source.payload.view!=1 ||
           !Fingerprint(input.source.source,&current))) {
            s_addresses.Invalidate(destination);return false;
        }
        verified.push_back({input.source,current,input.weight});
    }
    const bool copied=s_addresses.TransferBlend(verified,destination,output,[](const PoseIdentity& a,const PoseIdentity& b) {
        return a.view==1 && b.view==1 && a.poseId==b.poseId &&
               a.head.originSerial==b.head.originSerial &&
               std::memcmp(&a.localHead,&b.localHead,sizeof(a.localHead))==0;
    });
    if(copied) {
        const bool external=std::any_of(inputs.begin(),inputs.end(),[](const auto& i){return i.weight>0 && i.source.payload.externalView;});
        const auto receipt=s_addresses.Capture(destination,output);
        if(receipt && receipt.payload.externalView!=external) {
            auto value=receipt.payload;value.externalView=external;
            s_addresses.Publish(destination,output,value);
        }
    }
    return copied;
}
bool ReadContextPoseIdentity(uintptr_t context,PoseIdentity* out) {
    const auto camera=context+cvr::view_layout::kCamera;
    return context && (ReadCameraPoseIdentity(camera,1,out) || ReadCameraPoseIdentity(camera,2,out));
}
PoseIdentity CurrentNodePoseIdentity() { return t_nodePose; }
PoseIdentity SetNodePoseIdentity(const PoseIdentity& pose) { const auto previous=t_nodePose;t_nodePose=pose;return previous; }
}
