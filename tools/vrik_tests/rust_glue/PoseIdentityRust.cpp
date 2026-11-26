// tools/vrik_tests' copy of src/Camera/PoseIdentityRust.cpp as the plugin had it when the plugin's copy started losing what only these
// programs call (the C forms of the C++ the Rust solve reads, and the C++ names over the solve's Rust exports).
// The Rust variants (vrik_*_tests_rust) compile this file in place of the plugin's.
// The pose identity in Rust (CVR_RUST_VRIK: rust/crates/cvr-rig/src/vrik/pose_identity.rs, its ledger
// rust/crates/cvr-camera/src/pose_ledger.rs) in place of src/Camera/PoseIdentity.cpp: cvr::camera::* forwarded.
#include "Camera/PoseIdentity.hpp"
#include "Render/ViewCameraLayout.hpp"
#include <cstddef>

namespace cvr::camera {
static_assert(sizeof(OpenXRHeadPose) == 64 && offsetof(OpenXRHeadPose, originSerial) == 32 &&
              offsetof(OpenXRHeadPose, bodyBend) == 40 && offsetof(OpenXRHeadPose, afwEye) == 57,
              "OpenXRHeadPose is mirrored by HeadPose in rust/crates/cvr-rig/src/vrik/camera_link.rs");
static_assert(sizeof(PoseIdentity) == 136 && offsetof(PoseIdentity, view) == 24 && offsetof(PoseIdentity, head) == 32 &&
              offsetof(PoseIdentity, localHead) == 96 && offsetof(PoseIdentity, renderFrameId) == 124 &&
              offsetof(PoseIdentity, hasRenderFrameId) == 128 && offsetof(PoseIdentity, externalView) == 129,
              "PoseIdentity is mirrored in rust/crates/cvr-rig/src/vrik/pose_identity.rs");
static_assert(sizeof(PoseReceipt) == 184 && offsetof(PoseReceipt, generation) == 8 &&
              offsetof(PoseReceipt, fingerprint) == 16 && offsetof(PoseReceipt, payload) == 48,
              "PoseReceipt is mirrored by Receipt<PoseIdentity> in rust/crates/cvr-camera/src/pose_ledger.rs");
static_assert(sizeof(WeightedPoseReceipt) == 192 && offsetof(WeightedPoseReceipt, weight) == 184,
              "WeightedPoseReceipt is mirrored in rust/crates/cvr-rig/src/vrik/pose_identity.rs");
}

extern "C" {
uint64_t cvr_pose_next_identity();
void cvr_pose_invalidate(uintptr_t address);
void cvr_pose_capture(uintptr_t address,cvr::camera::PoseReceipt* out);
bool cvr_pose_transfer(const cvr::camera::PoseReceipt* source,uintptr_t destination);
void cvr_pose_publish_setup(uintptr_t address,uintptr_t component,uint32_t view,uint64_t poseId,const OpenXRHeadPose* head,
                            bool externalView);
void cvr_pose_publish_serialized(uintptr_t destination,const cvr::camera::PoseReceipt* source);
bool cvr_pose_read_camera(uintptr_t camera,uint32_t view,cvr::camera::PoseIdentity* out);
bool cvr_pose_publish_blended(uintptr_t destination,const cvr::camera::WeightedPoseReceipt* inputs,size_t count);
void cvr_pose_node_current(cvr::camera::PoseIdentity* out);
void cvr_pose_node_set(const cvr::camera::PoseIdentity* pose,cvr::camera::PoseIdentity* previous);
}

namespace cvr::camera {
uint64_t NextPoseIdentity() { return cvr_pose_next_identity(); }
void InvalidatePoseAddress(uintptr_t address) { cvr_pose_invalidate(address); }
PoseReceipt CapturePoseAddress(uintptr_t address) { PoseReceipt out{};cvr_pose_capture(address,&out);return out; }
bool TransferPoseAddress(const PoseReceipt& source,uintptr_t destination) { return cvr_pose_transfer(&source,destination); }
#if !CVR_RUST_VRIK
void PublishComponentPose(uintptr_t component,uint32_t view,uint64_t poseId,const OpenXRHeadPose& head) {
    PublishCameraSetupPose(component+0xE0,component,view,poseId,head);
}
#endif
void PublishCameraSetupPose(uintptr_t address,uintptr_t component,uint32_t view,uint64_t poseId,const OpenXRHeadPose& head,
                            bool externalView) {
    cvr_pose_publish_setup(address,component,view,poseId,&head,externalView);
}
void PublishSerializedPose(uintptr_t destination,const PoseReceipt& source) { cvr_pose_publish_serialized(destination,&source); }
bool ReadCameraPoseIdentity(uintptr_t camera,uint32_t view,PoseIdentity* out) { return cvr_pose_read_camera(camera,view,out); }
bool PublishBlendedPose(uintptr_t destination,std::span<const WeightedPoseReceipt> inputs) {
    return cvr_pose_publish_blended(destination,inputs.data(),inputs.size());
}
#if !CVR_RUST_VRIK
bool ReadContextPoseIdentity(uintptr_t context,PoseIdentity* out) {
    const auto camera=context+cvr::view_layout::kCamera;
    return context && (ReadCameraPoseIdentity(camera,1,out) || ReadCameraPoseIdentity(camera,2,out));
}
#endif
PoseIdentity CurrentNodePoseIdentity() { PoseIdentity out{};cvr_pose_node_current(&out);return out; }
PoseIdentity SetNodePoseIdentity(const PoseIdentity& pose) { PoseIdentity previous{};cvr_pose_node_set(&pose,&previous);return previous; }
}
