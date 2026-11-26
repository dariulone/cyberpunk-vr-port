// tools/vrik_tests' copy of src/Camera/CameraRust.cpp as the plugin had it when the plugin's copy started losing what only these
// programs call (the C forms of the C++ the Rust solve reads, and the C++ names over the solve's Rust exports).
// The Rust variants (vrik_*_tests_rust) compile this file in place of the plugin's.
// The camera link and the camera math in Rust (CVR_RUST_VRIK: rust/crates/cvr-rig/src/vrik/camera_link.rs and
// camera_math.rs) in place of src/Camera/CameraLink.cpp and CameraMath.cpp: the C++ API the hooks call, forwarded.
#include "Camera/CameraLink.hpp"
#include "Camera/CameraState.hpp"
#include "Core/CoreInternal.hpp"
#include <cstddef>

extern "C" {
void cvr_camera_anchor_recipe_publish(const cvr::camera::AnchorRecipe* recipe);
bool cvr_camera_anchor_recipe_read(cvr::camera::AnchorRecipe* recipe);
void cvr_camera_anchor_recipe_clear();
void cvr_camera_main_eye_centre_publish(const int32_t* eye,const int32_t* centre,const int32_t* bodyBase);
bool cvr_camera_main_eye_centre_read(const int32_t* eye,int32_t* centre,int32_t* bodyBase);
bool cvr_camera_main_eye_centre_read_world(const float* eyeOrCentre,float* centre,float* bodyBase);
void cvr_camera_cam_write_quat_publish(float x,float y,float z,float w,const OpenXRHeadPose* pose,const cvr::roomscale::Vec2* consumed,uint64_t sourceId);
bool cvr_camera_cam_write_quat_read(float* out,OpenXRHeadPose* pose,cvr::roomscale::Vec2* consumed,bool* consumedKnown,uint64_t* poseId);
void cvr_camera_cam_write_record_push(const float* q,const OpenXRHeadPose* pose);
bool cvr_camera_cam_write_record_find(const float* q,OpenXRHeadPose* out,uint32_t* age,uint32_t* ties);
bool cvr_camera_placed_pose_enabled();
void cvr_camera_placed_pose_push(uint32_t view,const float* q,const int32_t* position,const OpenXRHeadPose* pose);
bool cvr_camera_placed_pose_find(uint32_t view,const float* q,const int32_t* position,OpenXRHeadPose* out,uint32_t* age,uint32_t* ties);
void cvr_camera_located_frame_publish(const cvr::camera::LocatedCameraFrame* f);
bool cvr_camera_located_frame_read(cvr::camera::LocatedCameraFrame* out);
void cvr_camera_view_frame_publish(const cvr::camera::ViewFrame* f);
bool cvr_camera_view_frame_read(cvr::camera::ViewFrame* out);
void cvr_camera_barrel_frame_publish(const cvr::camera::BarrelFrame* f);
bool cvr_camera_barrel_frame_read(cvr::camera::BarrelFrame* out);
float cvr_camera_body_anchor_yaw(float fallback);
float cvr_camera_world_scale();
float cvr_camera_desired_half_ipd();
bool cvr_camera_plausible_unit_vector3(const float* v);
bool cvr_camera_plausible_unit_quaternion(const float* q);
bool cvr_camera_plausible_position_vec4(const float* v);
void cvr_camera_right_vector_from_quaternion(const float* q,float* outRight);
void cvr_camera_write_render_camera_basis(float* rsiPtr,const float* q);
void cvr_camera_apply_final_orientation_from_quat(float* rsiPtr,const float* q);
}
static_assert(sizeof(OpenXRHeadPose)==64 && offsetof(OpenXRHeadPose,valid)==28 && offsetof(OpenXRHeadPose,originSerial)==32 &&
              offsetof(OpenXRHeadPose,bodyBend)==40 && offsetof(OpenXRHeadPose,crouchDepth)==52 &&
              offsetof(OpenXRHeadPose,physicalCrouch)==56 && offsetof(OpenXRHeadPose,afwEye)==57,
              "OpenXRHeadPose is mirrored by HeadPose in rust/crates/cvr-rig/src/vrik/camera_link.rs");
static_assert(sizeof(cvr::camera::LocatedCameraFrame)==52 && sizeof(cvr::camera::ViewFrame)==56 &&
              sizeof(cvr::camera::BarrelFrame)==28 && sizeof(cvr::roomscale::Vec2)==8 &&
              sizeof(cvr::camera::AnchorRecipe)==44,
              "the camera link's packets are mirrored in rust/crates/cvr-rig/src/vrik/camera_link.rs");

namespace cvr::camera {
void AnchorRecipePublish(const AnchorRecipe& recipe) { cvr_camera_anchor_recipe_publish(&recipe); }
bool AnchorRecipeRead(AnchorRecipe* recipe) { return cvr_camera_anchor_recipe_read(recipe); }
void AnchorRecipeClear() { cvr_camera_anchor_recipe_clear(); }
void MainEyeCentrePublish(const int32_t eye[3],const int32_t centre[3],const int32_t bodyBase[3]) { cvr_camera_main_eye_centre_publish(eye,centre,bodyBase); }
bool MainEyeCentreRead(const int32_t eye[3],int32_t centre[3],int32_t bodyBase[3]) { return cvr_camera_main_eye_centre_read(eye,centre,bodyBase); }
bool MainEyeCentreReadWorld(const float eyeOrCentre[3],float centre[3],float bodyBase[3]) { return cvr_camera_main_eye_centre_read_world(eyeOrCentre,centre,bodyBase); }
void CamWriteQuatPublish(float x,float y,float z,float w,const OpenXRHeadPose& pose,const cvr::roomscale::Vec2* consumed,uint64_t sourceId) {
    cvr_camera_cam_write_quat_publish(x,y,z,w,&pose,consumed,sourceId);
}
bool CamWriteQuatRead(float out[4],OpenXRHeadPose* pose,cvr::roomscale::Vec2* consumed,bool* consumedKnown,uint64_t* poseId) {
    return cvr_camera_cam_write_quat_read(out,pose,consumed,consumedKnown,poseId);
}
void CamWriteRecordPush(const float q[4],const OpenXRHeadPose& p) { cvr_camera_cam_write_record_push(q,&p); }
bool CamWriteRecordFind(const float q[4],OpenXRHeadPose* out,uint32_t* outAge,uint32_t* outTies) { return cvr_camera_cam_write_record_find(q,out,outAge,outTies); }
bool PlacedCameraPoseEnabled() { return cvr_camera_placed_pose_enabled(); }
void PlacedCameraPosePush(uint32_t view,const float q[4],const int32_t position[3],const OpenXRHeadPose& pose) { cvr_camera_placed_pose_push(view,q,position,&pose); }
bool PlacedCameraPoseFind(uint32_t view,const float q[4],const int32_t position[3],OpenXRHeadPose* out,uint32_t* age,uint32_t* ties) {
    return cvr_camera_placed_pose_find(view,q,position,out,age,ties);
}
void LocatedCameraFramePublish(const LocatedCameraFrame& f) { cvr_camera_located_frame_publish(&f); }
bool LocatedCameraFrameRead(LocatedCameraFrame* out) { return cvr_camera_located_frame_read(out); }
void ViewFramePublish(const ViewFrame& f) { cvr_camera_view_frame_publish(&f); }
bool ViewFrameRead(ViewFrame* out) { return cvr_camera_view_frame_read(out); }
void BarrelFramePublish(const BarrelFrame& f) { cvr_camera_barrel_frame_publish(&f); }
bool BarrelFrameRead(BarrelFrame* out) { return cvr_camera_barrel_frame_read(out); }
float BodyAnchorYaw(float fallback) { return cvr_camera_body_anchor_yaw(fallback); }
}
float GetWorldScale() { return cvr_camera_world_scale(); }
float GetDesiredHalfIpd() { return cvr_camera_desired_half_ipd(); }
bool IsPlausibleUnitVector3(const float* v) { return cvr_camera_plausible_unit_vector3(v); }
bool IsPlausibleUnitQuaternion(const float* q) { return cvr_camera_plausible_unit_quaternion(q); }
bool IsPlausiblePositionVec4(const float* v) { return cvr_camera_plausible_position_vec4(v); }
void ComputeRightVectorFromQuaternion(const float* q,float* outRight) { cvr_camera_right_vector_from_quaternion(q,outRight); }
void WriteRenderCameraBasis(float* rsiPtr,const float* q) { cvr_camera_write_render_camera_basis(rsiPtr,q); }
void ApplyFinalCameraOrientationFromQuat(float* rsiPtr,const float* q) { cvr_camera_apply_final_orientation_from_quat(rsiPtr,q); }