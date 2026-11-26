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
#if !CVR_RUST_VRIK   // unused by the Rust build
void cvr_pose_publish_setup(uintptr_t address,uintptr_t component,uint32_t view,uint64_t poseId,const OpenXRHeadPose* head,
                            bool externalView);
#endif
void cvr_pose_node_current(cvr::camera::PoseIdentity* out);
}

namespace cvr::camera {
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
PoseIdentity CurrentNodePoseIdentity() { PoseIdentity out{};cvr_pose_node_current(&out);return out; }
#endif
}
