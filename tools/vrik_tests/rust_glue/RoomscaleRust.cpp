// tools/vrik_tests' copy of src/Hooks/RoomscaleRust.cpp as the plugin had it when the plugin's copy started losing what only these
// programs call (the C forms of the C++ the Rust solve reads, and the C++ names over the solve's Rust exports).
// The Rust variants (vrik_*_tests_rust) compile this file in place of the plugin's.
// The roomscale move, the body's heading and its follower in Rust (CVR_RUST_VRIK: rust/crates/cvr-rig/src/vrik/
// roomscale_move.rs, body_heading.rs, body_yaw_follow.rs) in place of the C++ in src/Hooks/RoomscaleMove.cpp,
// BodyHeading.cpp and BodyYawFollow.cpp: the C++ API and the C names the rest of the plugin calls, forwarded.
#include "Hooks/RoomscaleMove.hpp"
#include "Hooks/BodyHeading.hpp"
#include "Hooks/HookServices.hpp"
#include "Anim/VrikState.hpp"
#include "Camera/CameraState.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include "Runtimes/TrackedBodyYaw.hpp"

extern "C" {
void cvr_roomscale_set_player(uintptr_t player);
uintptr_t cvr_roomscale_player_identity();
bool cvr_roomscale_is_player_movement(const void* state);
bool cvr_roomscale_is_player_provider(const void* provider);
void cvr_roomscale_observe_movement(void* state);
bool cvr_roomscale_gameplay_allowed();
bool cvr_roomscale_physical_body_enabled();
bool cvr_roomscale_physical_body_heading_owned();
bool cvr_roomscale_physical_body_allowed();
bool cvr_roomscale_pose_frame_allowed();
cvr::roomscale::Vec2 cvr_roomscale_camera_consumed_at(uint64_t origin);
cvr::roomscale::Vec2 cvr_roomscale_camera_consumed();
bool cvr_roomscale_camera_rest_neck_at(uint64_t origin,cvr::roomscale::Vec2* out);
bool cvr_roomscale_camera_rest_neck(cvr::roomscale::Vec2* out);
bool cvr_roomscale_body_following();
bool cvr_body_heading_ready();
float cvr_body_centre_yaw(float fallback);
float cvr_body_tracking_yaw(float fallback);
bool cvr_body_read_tracking_yaw(float* yaw);
float cvr_body_heading_offset();
void cvr_body_heading_reset();
bool cvr_body_yaw_follow_active();
float cvr_body_yaw_follow_offset();
void cvr_body_yaw_follow_release();
float cvr_body_yaw_follow_step(float observedOffset);
void cvr_body_yaw_follow_tick(float engineZ,float engineW,const float* enginePos);
bool cvr_body_frame_read(float* p,float* q,float* trackingYaw);
void cvr_body_native_pair_publish(const int32_t* centre,const int32_t* bodyBase,const int32_t* entity,const float* cameraQuat,
                                  const float* entityQuat,uintptr_t owner,uint64_t origin,bool ownerStable);
bool cvr_body_native_pair_read(VrikTransformSnapshot* out);
}
static_assert(sizeof(cvr::roomscale::Vec2)==8 && sizeof(cvr::body::TrackingFrame)==112 &&
              offsetof(cvr::body::TrackingFrame,valid)==84 && offsetof(cvr::body::TrackingFrame,sequence)==88,
              "the roomscale and body-turn hooks' structures are mirrored in rust/crates/cvr-body and cvr-rig");

namespace cvr::roomscale {
void SetPlayer(uintptr_t player) { cvr_roomscale_set_player(player); }
uintptr_t PlayerIdentity() { return cvr_roomscale_player_identity(); }
bool IsPlayerMovement(const void* state) { return cvr_roomscale_is_player_movement(state); }
bool IsPlayerProvider(const void* provider) { return cvr_roomscale_is_player_provider(provider); }
void ObserveMovement(void* state) { cvr_roomscale_observe_movement(state); }
bool GameplayAllowed() { return cvr_roomscale_gameplay_allowed(); }
bool PhysicalBodyEnabled() { return cvr_roomscale_physical_body_enabled(); }
bool PhysicalBodyHeadingOwned() { return cvr_roomscale_physical_body_heading_owned(); }
bool PhysicalBodyAllowed() { return cvr_roomscale_physical_body_allowed(); }
bool PoseFrameAllowed() { return cvr_roomscale_pose_frame_allowed(); }
Vec2 CameraConsumed(uint64_t origin) { return cvr_roomscale_camera_consumed_at(origin); }
Vec2 CameraConsumed() { return cvr_roomscale_camera_consumed(); }
bool CameraRestNeck(uint64_t origin,Vec2* out) { return cvr_roomscale_camera_rest_neck_at(origin,out); }
bool CameraRestNeck(Vec2* out) { return cvr_roomscale_camera_rest_neck(out); }
bool BodyFollowing() { return cvr_roomscale_body_following(); }
}
bool BodyHeadingReady() { return cvr_body_heading_ready(); }
float BodyCentreYaw(float fallback) { return cvr_body_centre_yaw(fallback); }
float BodyTrackingYaw(float fallback) { return cvr_body_tracking_yaw(fallback); }
bool ReadBodyTrackingYaw(float* yaw) { return cvr_body_read_tracking_yaw(yaw); }
float BodyHeadingOffset() { return cvr_body_heading_offset(); }
void BodyHeadingReset() { cvr_body_heading_reset(); }
bool BodyYawFollowActive() { return cvr_body_yaw_follow_active(); }
float BodyYawFollowOffset() { return cvr_body_yaw_follow_offset(); }
extern "C" void BodyYawFollowRelease() { cvr_body_yaw_follow_release(); }
extern "C" float BodyYawFollowStep(float observedOffset) { return cvr_body_yaw_follow_step(observedOffset); }
extern "C" void BodyYawFollowTick(float engineZ,float engineW,const float* enginePos) { cvr_body_yaw_follow_tick(engineZ,engineW,enginePos); }
bool VRIK_ReadCurrentBodyFrame(float* p,float* q,float* trackingYaw) { return cvr_body_frame_read(p,q,trackingYaw); }
void VRIK_PublishNativeCameraPair(const int32_t* centre,const int32_t* bodyBase,const int32_t* entity,const float* cameraQuat,
                                  const float* entityQuat,uintptr_t owner,uint64_t origin,bool ownerStable) {
    cvr_body_native_pair_publish(centre,bodyBase,entity,cameraQuat,entityQuat,owner,origin,ownerStable);
}
bool VRIK_ReadNativeTransformSnapshot(VrikTransformSnapshot* out) { return cvr_body_native_pair_read(out); }