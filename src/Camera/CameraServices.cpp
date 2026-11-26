// What the camera link and camera math need from the rest of the plugin, in C form: the Rust port calls these
// (rust/crates/cvr-rig/src/vrik/camera_link.rs, camera_math.rs) and so does the C++ original, so tools/vrik_tests can
// stand in for all of them in both builds.
#include "Camera/CameraState.hpp"
#include "Camera/PoseIdentity.hpp"
#include "Core/LiveControls.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include <cstdint>

extern "C" {
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/cpp_services.rs
uint64_t cvr_cpp_xr_tracking_origin_serial() { return OpenXRManager::Get().GetTrackingOriginSerial(); }
#endif
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/cpp_services.rs
float cvr_cpp_xr_runtime_ipd() { return OpenXRManager::Get().GetRuntimeIpd(); }
#endif
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/cpp_services.rs
uint64_t cvr_cpp_next_pose_identity() { return cvr::camera::NextPoseIdentity(); }
#endif
// The pose identity's questions (src/Camera/PoseIdentity.cpp, or rust/crates/cvr-rig/src/vrik/pose_identity.rs).
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/cpp_services.rs
bool cvr_cpp_xr_resolve_rendered_pose(const OpenXRHeadPose* head,XrPosef* out) {
    return head && out && OpenXRManager::Get().ResolveRenderedPose(*head,out);
}
#endif
#if !CVR_RUST_VRIK   // unused by the Rust build
bool cvr_cpp_xr_external_pose_reset_pending() { return OpenXRManager::Get().ExternalPoseResetPending(); }
#endif
#if !CVR_RUST_VRIK   // the Rust build's is rust/crates/cvr-render/src/cpp_services.rs
bool cvr_cpp_device_cam_active() { return DeviceCamActive(); }
#endif
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/cpp_services.rs
bool cvr_cpp_braindance_active() { return g_bdActive.load(std::memory_order_relaxed) != 0; }
#endif
// g_liveControls' world, IPD and stereo scales.
#if !CVR_RUST_VRIK   // unused by the Rust build
void cvr_cpp_camera_scales(float* out) {
    out[0]=g_liveControls.xrWorldScale;out[1]=g_liveControls.xrIpdScale;out[2]=g_liveControls.xrStereoScale;
}
#endif
}