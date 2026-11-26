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
uintptr_t cvr_roomscale_player_identity();
bool cvr_roomscale_is_player_provider(const void* provider);
cvr::roomscale::Vec2 cvr_roomscale_camera_consumed_at(uint64_t origin);
cvr::roomscale::Vec2 cvr_roomscale_camera_consumed();
bool cvr_roomscale_camera_rest_neck_at(uint64_t origin,cvr::roomscale::Vec2* out);
bool cvr_roomscale_camera_rest_neck(cvr::roomscale::Vec2* out);
bool cvr_body_frame_read(float* p,float* q,float* trackingYaw);
void cvr_body_native_pair_publish(const int32_t* centre,const int32_t* bodyBase,const int32_t* entity,const float* cameraQuat,
                                  const float* entityQuat,uintptr_t owner,uint64_t origin,bool ownerStable);
bool cvr_body_native_pair_read(VrikTransformSnapshot* out);
}
static_assert(sizeof(cvr::roomscale::Vec2)==8 && sizeof(cvr::body::TrackingFrame)==112 &&
              offsetof(cvr::body::TrackingFrame,valid)==84 && offsetof(cvr::body::TrackingFrame,sequence)==88,
              "the roomscale and body-turn hooks' structures are mirrored in rust/crates/cvr-body and cvr-rig");

namespace cvr::roomscale {
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
uintptr_t PlayerIdentity() { return cvr_roomscale_player_identity(); }
bool IsPlayerProvider(const void* provider) { return cvr_roomscale_is_player_provider(provider); }
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
Vec2 CameraConsumed(uint64_t origin) { return cvr_roomscale_camera_consumed_at(origin); }
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
Vec2 CameraConsumed() { return cvr_roomscale_camera_consumed(); }
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
bool CameraRestNeck(uint64_t origin,Vec2* out) { return cvr_roomscale_camera_rest_neck_at(origin,out); }
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
bool CameraRestNeck(Vec2* out) { return cvr_roomscale_camera_rest_neck(out); }
#endif
}
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's is rust/crates/cvr-render/src/cpp_services.rs'
bool VRIK_ReadCurrentBodyFrame(float* p,float* q,float* trackingYaw) { return cvr_body_frame_read(p,q,trackingYaw); }
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
void VRIK_PublishNativeCameraPair(const int32_t* centre,const int32_t* bodyBase,const int32_t* entity,const float* cameraQuat,
                                  const float* entityQuat,uintptr_t owner,uint64_t origin,bool ownerStable) {
    cvr_body_native_pair_publish(centre,bodyBase,entity,cameraQuat,entityQuat,owner,origin,ownerStable);
}
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's is rust/crates/cvr-render/src/cpp_services.rs'
bool VRIK_ReadNativeTransformSnapshot(VrikTransformSnapshot* out) { return cvr_body_native_pair_read(out); }
#endif