// What the Rust hooks (rust/crates/cvr-rig/src/vrik) and the C++ hooks under test need from the rest of the plugin, in
// C form: OpenXR, the camera state, the live controls. Both builds; tools/vrik_tests replaces every one of them.
#include "Hooks/HookServices.hpp"
#include "Hooks/RoomscaleMove.hpp"
#include "Hooks/SwimmingInput.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include "Camera/CameraState.hpp"
#include "Core/LiveControls.hpp"
#include "Overlay/LiveControlsUi.hpp"
// The Rust build's LiveControls and LiveControlsUiState are rust/crates/cvr-settings' mirrors; the C++ compiled into
// it reads them through these headers.
static_assert(sizeof(LiveControls) == 440, "mirrored by rust/crates/cvr-settings/src/live_controls_block.rs");
static_assert(sizeof(LiveControlsUiState) == 1516, "mirrored by rust/crates/cvr-settings/src/live_controls_ui_state.rs");
#include "Core/VrCoreShared.hpp"
#include "Anim/VrikState.hpp"
#include "Core/Telemetry.hpp"
#include "Overlay/ImGuiOverlay.hpp"
#include "Utils/DebugGate.hpp"
#include "Utils/WeakObjectSlot.hpp"
#include "Camera/PlayerCameraState.hpp"
#include "Lobby/MenuLobby.hpp"
#include "Stereo/StereoInternal.hpp"
#include "Fbt/Game/Body.hpp"
#include "Render/Afw.hpp"
#include "Camera/GazeCenter.hpp"
#include "Natives/Reach.hpp"
#include <RED4ext/Scripting/Natives/entEntity.hpp>
#include "Camera/SurveillanceFollow.hpp"
#include <RED4ext/Scripting/Natives/entIComponent.hpp>
#include <RED4ext/RED4ext.hpp>

bool EnsureCameraPoseCopyHook();   // src/Hooks/WeaponAim.cpp
namespace {
// CameraPoseTransport's second-eye camera: a weak reference to VRCAM's component, bound only from the engine's RTT
// callbacks (which own it), locked to notify it of a completed MAIN. RED4ext's handles stay C++ until the engine's
// reference counting is in Rust.
#if !CVR_RUST_VRIK   // the Rust build's slot is rust/crates/cvr-ffi/src/hook_services.rs's
cvr::WeakObjectSlot<RED4ext::WeakHandle<RED4ext::ISerializable>> s_externalVrcam;
#endif
}

extern "C" {
#if !CVR_RUST_VRIK   // the Rust build calls the XR runtime's Rust function itself (rust/crates/cvr-rig/src/vrik/xr_calls.rs)
bool cvr_cpp_xr_roomscale_sample(cvr::roomscale::Sample* out,OpenXRHeadPose* frameHead,XrTime* frameTime) {
    return OpenXRManager::Get().GetRoomscaleSample(out,frameHead,frameTime);
}
#endif
#if !CVR_RUST_VRIK   // the Rust build calls the XR runtime's Rust function itself (rust/crates/cvr-rig/src/vrik/xr_calls.rs)
void cvr_cpp_xr_flush_hands_to_shared(const OpenXRHeadPose* frameHead,XrTime frameTime,const cvr::roomscale::Vec2* consumed) {
    OpenXRManager::Get().FlushHandsToShared(frameHead,frameTime,consumed);
}
#endif
// The same input latch the CCT move reads (not the completed previous move camera and IK consumers still own).
#if !CVR_RUST_VRIK   // the Rust build calls the XR runtime's Rust function itself (rust/crates/cvr-rig/src/vrik/xr_calls.rs)
bool cvr_cpp_xr_acquire_frame_head(OpenXRHeadPose* out) {
    return OpenXRManager::Get().AcquireFrameHeadSample(out,nullptr,nullptr,nullptr,false);
}
#endif
#if !CVR_RUST_VRIK   // the Rust build calls the XR runtime's Rust function itself (rust/crates/cvr-rig/src/vrik/xr_calls.rs)
bool cvr_cpp_xr_body_tracking_frame(cvr::body::TrackingFrame* out) { return OpenXRManager::Get().GetBodyTrackingFrame(out); }
#endif
// A player replacement: a previous puppet can keep animating, its camera counters still advancing.
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/cpp_services.rs
void cvr_cpp_camera_player_changed() { g_camObjMain.store(0,std::memory_order_release);PatchFastDisarm(); }
#endif
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/cpp_services.rs
int cvr_cpp_menu_mode() { return g_menuModeValue; }
#endif
// The foreground window is this process's: the cyberware chord's key reaches no other app.
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/cpp_services.rs
bool cvr_cpp_foreground_in_process() {
    DWORD process{};GetWindowThreadProcessId(GetForegroundWindow(),&process);return process==GetCurrentProcessId();
}
#endif
// The headset is on and this game has its input (BackgroundPad).
// FinalCamera's telemetry, its rendered-frame poses (MAIN's queue, or VRCAM's), and MAIN's placed camera
// (WorldSpaceBlendCamera's source).
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/cpp_services.rs
void cvr_cpp_telemetry_final(uint32_t hits,uintptr_t rsi) {
    if(g_telemetry) { g_telemetry->finalHits=hits;g_telemetry->finalRsi=rsi; }
}
#endif
#if !CVR_RUST_VRIK   // the Rust build calls the XR runtime's Rust function itself (rust/crates/cvr-rig/src/vrik/xr_calls.rs)
void cvr_cpp_xr_push_rendered_frame_pose(const OpenXRHeadPose* pose,bool vrcam) {
    if(!pose)return;
    if(vrcam)OpenXRManager::Get().PushVrcamRenderedFramePose(*pose);else OpenXRManager::Get().PushRenderedFramePose(*pose);
}
#endif
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/cpp_services.rs
uintptr_t cvr_cpp_cam_obj_main() { return g_camObjMain.load(std::memory_order_acquire); }
#endif
// CameraPoseTransport's questions (src/Hooks/CameraPoseTransport.cpp, or rust/crates/cvr-rig/src/vrik/pose_transport.rs).
#if !CVR_RUST_VRIK   // the Rust build's is rust/crates/cvr-render/src/cpp_services.rs
bool cvr_cpp_lobby_world_shown() { return cvr::lobby::WorldShown(); }
#endif
#if !CVR_RUST_VRIK   // the Rust build calls the XR runtime's Rust function itself (rust/crates/cvr-rig/src/vrik/xr_calls.rs)
bool cvr_cpp_xr_session_running() { return OpenXRManager::Get().IsSessionRunning(); }
#endif
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/cpp_services.rs
bool cvr_cpp_bd_scene_pose_valid() { return g_bdScenePoseValid.load(std::memory_order_acquire) != 0; }
#endif
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/cpp_services.rs
uintptr_t cvr_cpp_vrcam_comp() { return cvr::detail::g_vrcam_comp.load(std::memory_order_acquire); }
#endif
#if !CVR_RUST_VRIK   // the Rust build calls the XR runtime's Rust function itself (rust/crates/cvr-rig/src/vrik/xr_calls.rs)
bool cvr_cpp_xr_acquire_camera_pose_frame(OpenXRHeadPose* head,cvr::roomscale::Vec2* consumed,uint64_t* sequence) {
    return OpenXRManager::Get().AcquireCameraPoseFrame(head,consumed,sequence);
}
#endif
#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-ffi/src/player_camera_state.rs
void cvr_cpp_refresh_player_camera_state() { RefreshPlayerCameraState(); }
#endif
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/cpp_services.rs
float cvr_cpp_normal_fov_override() { return g_normalFovOverrideValue; }
#endif
#if !CVR_RUST_VRIK   // the Rust build's is rust/crates/cvr-render/src/cpp_services.rs
bool cvr_cpp_ensure_camera_pose_copy_hook() { return EnsureCameraPoseCopyHook(); }
#endif
// PatchCamera's questions (src/Hooks/PatchCamera.cpp, or rust/crates/cvr-rig/src/vrik/patch_camera.rs): the owner
// classification, the device camera and the braindance, the scene comfort, full-body tracking, AFW, OpenXR's pose
// samples and the fast-path block and telemetry line the trampoline bakes in.
#if !CVR_RUST_VRIK   // the Rust build calls the Rust function itself (cvr-render / cvr-ffi)
int cvr_cpp_classify_patch_camera_owner(void* owner) { return ClassifyPatchCameraOwner(owner); }
#endif
#if !CVR_RUST_VRIK   // the Rust build calls the Rust function itself (cvr-render / cvr-ffi)
void cvr_cpp_refresh_lens_from_component() { RefreshLensFromComponent(); }
#endif
#if !CVR_RUST_VRIK   // the Rust build calls the Rust function itself (cvr-render / cvr-ffi)
bool cvr_cpp_braindance_camera_match(uintptr_t object,const float* quat) { return BraindanceCameraMatch(object,quat); }
#endif
#if !CVR_RUST_VRIK   // the Rust build calls the Rust function itself (cvr-render / cvr-ffi)
bool cvr_cpp_locate_owns_takeover() { return LocateOwnsTakeover(); }
#endif
#if !CVR_RUST_VRIK   // the Rust build calls the Rust function itself (cvr-render / cvr-ffi)
bool cvr_cpp_bd_push_owns_composition() { return BdPushOwnsComposition(); }
#endif
#if !CVR_RUST_VRIK   // the Rust build's is rust/crates/cvr-render/src/cpp_services.rs
bool cvr_cpp_scene_comfort_apply(const float* raw,float* out) { return SceneComfortApply(raw,out); }
#endif
#if !CVR_RUST_VRIK   // the Rust build calls the Rust function itself (cvr-render / cvr-ffi)
void cvr_cpp_push_lens_head_transform(const float* quat) { PushLensHeadTransform(quat); }
#endif
#if !CVR_RUST_VRIK   // the Rust build's is rust/crates/cvr-render/src/cpp_services.rs
bool cvr_cpp_surveillance_view_yaw(uintptr_t lens,uintptr_t owner,uint64_t origin,float ownerYaw,float* yaw) {
    return yaw && cvr::camera::SurveillanceViewYaw(lens,owner,origin,ownerYaw,*yaw);
}
#endif
bool cvr_cpp_fbt_camera_eye_height(float* z) { return z && cvr::fbt::game::CameraEyeHeight(z); }
#if !CVR_RUST_VRIK   // the Rust build calls the Rust function itself (cvr-render / cvr-ffi)
bool cvr_cpp_afw_active() { return cvr::afw::Active(); }
#endif
#if !CVR_RUST_VRIK   // the Rust build calls the Rust function itself (cvr-render / cvr-ffi)
int cvr_cpp_afw_frame_eye() { return cvr::afw::FrameEye(); }
#endif
#if !CVR_RUST_VRIK   // the Rust build reads the live controls block itself (rust/crates/cvr-rig/src/vrik/live_reads.rs)
float cvr_cpp_scene_cam_comfort_deg() { return g_liveControls.xrSceneCamComfortDeg; }
#endif
#if !CVR_RUST_VRIK   // the Rust build calls the XR runtime's Rust function itself (rust/crates/cvr-rig/src/vrik/xr_calls.rs)
uint64_t cvr_cpp_xr_frame_aim_epoch() { return OpenXRManager::Get().GetFrameAimEpoch(); }
#endif
#if !CVR_RUST_VRIK   // the Rust build calls the XR runtime's Rust function itself (rust/crates/cvr-rig/src/vrik/xr_calls.rs)
int64_t cvr_cpp_xr_frame_aim_time() { return OpenXRManager::Get().GetFrameAimTime(); }
#endif
#if !CVR_RUST_VRIK   // the Rust build calls the XR runtime's Rust function itself (rust/crates/cvr-rig/src/vrik/xr_calls.rs)
bool cvr_cpp_xr_locate_head_pose_at(int64_t time,OpenXRHeadPose* out) { return OpenXRManager::Get().LocateHeadPoseAt(time,out); }
#endif
#if !CVR_RUST_VRIK   // the Rust build calls the XR runtime's Rust function itself (rust/crates/cvr-rig/src/vrik/xr_calls.rs)
bool cvr_cpp_xr_get_head_pose(OpenXRHeadPose* out) { return OpenXRManager::Get().GetHeadPose(out); }
#endif
#if !CVR_RUST_VRIK   // unused by the Rust build
void cvr_cpp_xr_push_render_head_pose(const OpenXRHeadPose* pose) { if(pose)OpenXRManager::Get().PushRenderHeadPose(*pose); }
#endif
#if !CVR_RUST_VRIK   // the Rust build calls the XR runtime's Rust function itself (rust/crates/cvr-rig/src/vrik/xr_calls.rs)
bool cvr_cpp_xr_acquire_frame_head_sample(OpenXRHeadPose* out) { return OpenXRManager::Get().AcquireFrameHeadSample(out); }
#endif
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/cpp_services.rs
uintptr_t cvr_cpp_patch_fast_path() { return reinterpret_cast<uintptr_t>(&g_patchFast); }
#endif
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/cpp_services.rs
uintptr_t cvr_cpp_telemetry_patch_hits() { return g_telemetry ? reinterpret_cast<uintptr_t>(g_telemetry)+kPatchTelemetryOffset:0; }
#endif
// LocateCamera's (src/Hooks/LocateCamera.cpp, or rust/crates/cvr-rig/src/vrik/locate_camera.rs): its telemetry line, the
// head offsets the settings hold, and OpenXR's render eye, eye-pose store, baked camera offset and hands.
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/cpp_services.rs
void cvr_cpp_telemetry_locate(uint32_t hits,uint64_t rbx,float xmm0) {
    if(!g_telemetry)return;
    g_telemetry->locateHits=hits;g_telemetry->locateRbx=static_cast<uintptr_t>(rbx);g_telemetry->locateXmm0=xmm0;
}
#endif
#if !CVR_RUST_VRIK   // the Rust build reads the live controls block itself (rust/crates/cvr-rig/src/vrik/live_reads.rs)
void cvr_cpp_camera_offsets(float* out) {
    out[0]=g_liveControls.xrHeadOffsetX;out[1]=g_liveControls.xrHeadOffsetY;out[2]=g_liveControls.xrHeadOffsetZ;
    out[3]=g_liveControls.xrVehHeadOffsetX;out[4]=g_liveControls.xrVehHeadOffsetY;out[5]=g_liveControls.xrVehHeadOffsetZ;
}
#endif
#if !CVR_RUST_VRIK   // unused by the Rust build
int cvr_cpp_xr_render_eye_index() { return OpenXRManager::Get().GetCurrentRenderEyeIndex(); }
#endif
#if !CVR_RUST_VRIK   // the Rust build calls the XR runtime's Rust function itself (rust/crates/cvr-rig/src/vrik/xr_calls.rs)
void cvr_cpp_xr_store_render_eye_pose(int eye,const OpenXRHeadPose* pose,uint32_t seq) {
    if(pose)OpenXRManager::Get().StoreRenderEyePose(eye,*pose,seq);
}
#endif
#if !CVR_RUST_VRIK   // unused by the Rust build
void cvr_cpp_xr_camera_offset(float* out) { OpenXRManager::Get().GetCameraOffset(out); }
#endif
#if !CVR_RUST_VRIK   // the Rust build calls the XR runtime's Rust function itself (rust/crates/cvr-rig/src/vrik/xr_calls.rs)
bool cvr_cpp_xr_hand_pose(int hand,OpenXRHeadPose* out) { return OpenXRManager::Get().GetHandPose(hand,out); }
#endif
// WeaponAim's (src/Hooks/WeaponAim.cpp, or rust/crates/cvr-rig/src/vrik/weapon_aim.rs): the device takeover and the
// binocular gaze centre.
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/cpp_services.rs
bool cvr_cpp_remote_cam_on() { return g_remoteCamOn.load(std::memory_order_acquire)!=0; }
#endif
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/cpp_services.rs
uint64_t cvr_cpp_takeover_entity_id() { return g_takeoverEntityId.load(std::memory_order_acquire); }
#endif
#if !CVR_RUST_VRIK   // unused by the Rust build
float cvr_cpp_gaze_tangent_y() { return cvr::gaze::Read().tangentY; }
#endif
// The tank hooks' (src/Hooks/PanzerSteering.cpp / PanzerWeaponAim.cpp, or rust/crates/cvr-rig/src/vrik/panzer.rs): the
// mounted vehicle and whether it is a tank (RTTI).
#if !CVR_RUST_VRIK   // the Rust build calls the Rust function itself (cvr-render / cvr-ffi)
uintptr_t cvr_cpp_mounted_vehicle_identity() { return CurrentMountedVehicleIdentity(); }
#endif
// The menu mode hook's (src/Hooks/MenuModeCallback.cpp, or rust/crates/cvr-rig/src/vrik/menu_mode.rs): the lobby holding
// the menu up while it renders behind it.
// SurveillanceBody's (src/Hooks/SurveillanceBody.cpp, or rust/crates/cvr-rig/src/vrik/surveillance_body.rs): the device
// camera control component's owner, entity and kind.
struct SurveillanceControl {uintptr_t owner{};uint64_t entity{};bool enabled{};uint8_t kind{};};
#if !CVR_RUST_VRIK   // unused by the Rust build
bool cvr_cpp_surveillance_control(uintptr_t component,SurveillanceControl* out) {
    if(!component || !out)return false;
    auto* control=reinterpret_cast<RED4ext::ent::IComponent*>(component);
    *out={};out->owner=reinterpret_cast<uintptr_t>(control->owner);out->enabled=control->isEnabled;
    if(!control->owner)return true;
    out->entity=control->owner->entityID.hash;
    const auto type=control->owner->GetType()->GetName();
    out->kind=type==RED4ext::CName("SniperNest") ? 2:type==RED4ext::CName("SurveillanceCamera") ? 1:0;
    return true;
}
#endif
// StoryAttention's (src/Hooks/StoryAttention.cpp, or rust/crates/cvr-rig/src/vrik/story_attention.rs): the hand's reach.
#if !CVR_RUST_VRIK   // the Rust build's is rust/crates/cvr-render/src/cpp_services.rs
bool cvr_cpp_reach_aim_point(float* point) { return point && cvr::reach::AimPoint(point); }
bool cvr_cpp_lobby_holds_render(uintptr_t menuState) { return cvr::lobby::HoldsRender(reinterpret_cast<const void*>(menuState)); }
#endif
#if !CVR_RUST_VRIK   // unused by the Rust build
bool cvr_cpp_is_tank(uintptr_t owner) {
    if(!owner)return false;
    static const auto* tankClass=RED4ext::CRTTISystem::Get()->GetClass("vehicleTankBaseObject");
    return tankClass && reinterpret_cast<RED4ext::ISerializable*>(owner)->GetType()->IsA(tankClass);
}
#endif
// Only called with the live argument of the engine's RTT callbacks.
#if !CVR_RUST_VRIK   // unused by the Rust build
void cvr_cpp_external_vrcam_observe(uintptr_t component) {
    if(!component)return;
    const auto* object=reinterpret_cast<RED4ext::ent::IComponent*>(component);
    if(object->ref.instance==object && object->ref.refCount)
        s_externalVrcam.Bind(component,reinterpret_cast<uintptr_t>(object->ref.refCount),object->ref);
}
#endif
// 0: notified, the component held alive through the call; 1: expired or replaced; 2: disabled, detached, or no longer
// the second eye's camera.
#if !CVR_RUST_VRIK   // unused by the Rust build
int cvr_cpp_external_vrcam_notify(uintptr_t eyeCamera,void(*notify)(uintptr_t)) {
    const auto live=s_externalVrcam.Lock(eyeCamera);
    if(!live)return 1;
    const auto* component=static_cast<RED4ext::ent::IComponent*>(live.GetPtr());
    if(!component->isEnabled || !component->owner || eyeCamera!=cvr::detail::g_vrcam_comp.load(std::memory_order_acquire))return 2;
    notify(eyeCamera);
    return 0;
}
#endif
#if !CVR_RUST_VRIK   // the Rust build calls the XR runtime's Rust function itself (rust/crates/cvr-rig/src/vrik/xr_calls.rs)
bool cvr_cpp_session_focused() { return OpenXRManager::Get().IsSessionFocused(); }
#endif
#if !CVR_RUST_VRIK   // the Rust build's is rust/crates/cvr-render/src/cpp_services.rs
bool cvr_cpp_swimming_read_steering(float* direction,int* state) {
    XrVector3f d{};const bool ok=cvr::swimming::ReadSteering(&d,state);
    direction[0]=d.x;direction[1]=d.y;direction[2]=d.z;return ok;
}
#endif
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/rig_services.rs
bool cvr_cpp_pose_heading_ready() { return g_PlayerPoseHeadingReady.load(std::memory_order_acquire); }
#endif
#if !CVR_RUST_VRIK   // the Rust build reads the live controls block itself (rust/crates/cvr-rig/src/vrik/live_reads.rs)
void cvr_cpp_body_controls(CvrBodyControls* out) {
    out->roomscaleMovement=g_liveControls.xrRoomscaleMovement;out->cutsceneSuspendTier=g_liveControls.xrCutsceneSuspendTier;
    out->bodyRotationMode=g_liveControls.xrBodyRotationMode;out->bodyMoveRadius=g_liveControls.xrBodyMoveRadius;
    out->bodyFreeLookDeg=g_liveControls.xrBodyFreeLookDeg;out->bodyFreeLookDownDeg=g_liveControls.xrBodyFreeLookDownDeg;
    out->bodyFreeLookSwimDeg=g_liveControls.xrBodyFreeLookSwimDeg;
}
#endif
// The on-foot input hooks (rust/crates/cvr-rig/src/vrik/free_delta_head.rs, on_foot_move_xy.rs, on_foot_delta_head.rs).
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/cpp_services.rs
uintptr_t cvr_cpp_telemetry_free_delta() { return reinterpret_cast<uintptr_t>(g_telemetry)+kFreeDeltaTelemetryOffset; }
#endif
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/cpp_services.rs
void cvr_cpp_telemetry_move_xy(void* moveStruct) {
    if(!g_telemetry)return;
    CVR_DIAGNOSTIC(++g_telemetry->moveXYHits);
    CVR_DIAGNOSTIC(g_telemetry->moveXYRsi=reinterpret_cast<uintptr_t>(moveStruct));
}
#endif
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/cpp_services.rs
void cvr_cpp_telemetry_delta_head(float* deltaHead) {
    if(!g_telemetry)return;
    ++g_telemetry->deltaHeadHits;g_telemetry->deltaHeadRcx=reinterpret_cast<uintptr_t>(deltaHead);
}
#endif
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/cpp_services.rs
int32_t cvr_cpp_take_pending_snap_yaw() { return InterlockedExchange(&g_pendingSnapYawDeltaBits,0); }
#endif
#if !CVR_RUST_VRIK   // the Rust build's is rust/crates/cvr-render/src/cpp_services.rs
float* cvr_cpp_shot_shared() { return GetShotShared(); }
void cvr_cpp_swimming_bind_native_input(void* locomotion) { cvr::swimming::BindNativeInput(locomotion); }
#endif
#if !CVR_RUST_VRIK   // the Rust build reads the live controls block itself (rust/crates/cvr-rig/src/vrik/live_reads.rs)
int cvr_cpp_movement_source() { return g_liveControls.xrMovementSource; }
#endif
#if !CVR_RUST_VRIK   // the Rust build calls the XR runtime's Rust function itself (rust/crates/cvr-rig/src/vrik/xr_calls.rs)
float cvr_cpp_xr_hmd_yaw_rel_to_body() { return OpenXRManager::Get().GetHmdYawRelToBody(); }
#endif
#if !CVR_RUST_VRIK   // the Rust build calls the XR runtime's Rust function itself (rust/crates/cvr-rig/src/vrik/xr_calls.rs)
float cvr_cpp_xr_hand_yaw_rel_to_body(int side) { return OpenXRManager::Get().GetHandYawRelToBody(side); }
#endif
// The keys the XInput merge types (the scanner's zoom and list): here, not in InputServices.cpp, so the bench can record
// them instead.
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/cpp_services.rs
uint32_t cvr_cpp_send_input(uint32_t count, const void* inputs, int size) {
    return SendInput(count, static_cast<INPUT*>(const_cast<void*>(inputs)), size);
}
#endif
// The ladder and the swim strokes (rust/crates/cvr-rig/src/vrik/ladder_input.rs, swimming_input.rs).
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/cpp_services.rs
bool cvr_cpp_overlay_visible() { return OverlayIsVisible(); }
#endif
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/cpp_services.rs
bool cvr_cpp_simulator_loaded() { return GetModuleHandleW(L"openxr_simulator.dll") != nullptr; }
#endif
#if !CVR_RUST_VRIK   // unused by the Rust build
struct CvrLadderControls { int32_t gripClimb, autoFinish; float finishDistance; };
#endif
#if !CVR_RUST_VRIK   // the Rust build reads the live controls block itself (rust/crates/cvr-rig/src/vrik/live_reads.rs)
void cvr_cpp_ladder_controls(CvrLadderControls* out) {
    out->gripClimb=g_liveControls.xrLadderGripClimb;out->autoFinish=g_liveControls.xrLadderAutoFinish;
    out->finishDistance=g_liveControls.xrLadderFinishDistance;
}
#endif
#if !CVR_RUST_VRIK   // the Rust build reads the live controls block itself (rust/crates/cvr-rig/src/vrik/live_reads.rs)
int cvr_cpp_breaststroke_swim() { return g_liveControls.xrBreaststrokeSwim; }
#endif
#if !CVR_RUST_VRIK   // the Rust build's is rust/crates/cvr-render/src/cpp_services.rs
bool cvr_cpp_swimming_script_hooks_active() { return cvr::swimming::ScriptHooksActive(); }
#endif
// SEH AROUND THE ENGINE'S OWN GETTER, which Rust cannot catch: the context the locomotion sampler just used, when its
// provider is GetPlayer()'s.
#if !CVR_RUST_VRIK   // unused by the Rust build
uintptr_t cvr_cpp_swimming_player_input_object(void* locomotion) {
    __try {
        if(!locomotion)return 0;
        const auto wrapper=static_cast<uint8_t*>(locomotion)+0x30;
        const auto table=*reinterpret_cast<uintptr_t*>(wrapper);
        if(!table)return 0;
        const auto getter=*reinterpret_cast<uintptr_t*>(table);
        if(!getter)return 0;
        const auto context=reinterpret_cast<uintptr_t(__fastcall*)(void*)>(getter)(wrapper);
        if(!context || !cvr::roomscale::IsPlayerProvider(*reinterpret_cast<void**>(context+8)))return 0;
        return *reinterpret_cast<uintptr_t*>(context+0x10);
    } __except(EXCEPTION_EXECUTE_HANDLER) { return 0; }
}
#endif
}