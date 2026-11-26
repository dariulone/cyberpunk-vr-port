// The player's solve in Rust (CVR_RUST_VRIK, rust/crates/cvr-rig/src/vrik) in place of src/Anim/CharacterRig.cpp:
// the C++ the Rust side calls -- members, namespaced functions, the log -- in C form, and the solve's namespaced
// readers the rest of the plugin calls, forwarded to their Rust definitions.
#include "Anim/CharacterRig.hpp"
#include "Anim/HandGestures.hpp"
#include "Anim/LeftHanded.hpp"
#include "Anim/ReloadPose.hpp"
#include "Anim/Cyberware.hpp"
#include "Anim/HeadAimWeapon.hpp"
#include "Anim/AdsEyeAlign.hpp"
#include "Anim/AdsMuzzleStabilizer.hpp"
#include "Anim/FkRecorder.hpp"
#include "Anim/SmokingPose.hpp"
#include "Anim/ReloadHandOff.hpp"
#include "Anim/WeaponRig.hpp"
#include "Anim/GraspPose.hpp"
#include "Anim/TwoHandGrip.hpp"
#include "Anim/WheelGrab.hpp"
#include "Anim/TrackedWheelSteering.hpp"
#include "Core/LiveControls.hpp"
#include "Core/VrCoreShared.hpp"
#include "Overlay/ImGuiOverlay.hpp"
#include "Runtimes/FingerGrip.hpp"
#include "Runtimes/Haptics.hpp"
#include "Camera/CameraState.hpp"
#include "Camera/CameraLink.hpp"
#include "Camera/PoseIdentity.hpp"
#include "Hooks/LadderInput.hpp"
#include "Hooks/RoomscaleMove.hpp"
#include "Hooks/SwimmingInput.hpp"
#include "Lobby/MenuLobby.hpp"
#include "Fbt/Game/Calibration.hpp"
#include "Utils/DebugGate.hpp"
#include "Camera/NeckCameraMount.hpp"
#include "Core/Paths.hpp"
#include "Natives/NativeState.hpp"  // VRGripPath
#include "Fbt/Game/Body.hpp"
#include "Hooks/BodyHeading.hpp"
#include "Runtimes/OpenXRManager.hpp"

#include <cstddef>
#include <cstdio>
#include <cstring>
#include <share.h>
#include <string>

void Log(const char* fmt, ...);

// The finger layers (src/Anim/ReloadPose.cpp in Rust: rust/crates/cvr-rig/src/vrik/reload.rs).

// The finger gestures and the arm cyberware (src/Anim/HandGestures.cpp and Cyberware.cpp in Rust:
// rust/crates/cvr-rig/src/vrik/gestures.rs and cyberware.rs).
static_assert(sizeof(cvr::cyberware::Frame)==1208 && sizeof(cvr::cyberware::Plane)==28,
              "cvr::cyberware::Frame and Plane are shared with rust/crates/cvr-rig/src/vrik/cyberware.rs");
// Finger tracking from the hand skeleton (src/Runtimes/FingerGrip.cpp in Rust: rust/crates/cvr-rig/src/vrik/fingergrip.rs).
// The extension list and the loader's xrGetInstanceProcAddr stay here; the structures Rust builds are openxr.h's.
extern "C" {
bool cvr_fingergrip_grip_from_fingers();
}
static_assert(sizeof(XrHandJointLocationEXT)==40 && offsetof(XrHandJointLocationEXT,pose)==8 &&
              offsetof(XrHandJointLocationEXT,radius)==36 && sizeof(XrHandJointLocationsEXT)==32 &&
              offsetof(XrHandJointLocationsEXT,isActive)==16 && offsetof(XrHandJointLocationsEXT,jointLocations)==24 &&
              sizeof(XrHandJointsLocateInfoEXT)==32 && offsetof(XrHandJointsLocateInfoEXT,baseSpace)==16 &&
              offsetof(XrHandJointsLocateInfoEXT,time)==24 && sizeof(XrHandJointsMotionRangeInfoEXT)==24 &&
              sizeof(XrHandTrackerCreateInfoEXT)==24 && offsetof(XrHandTrackerCreateInfoEXT,handJointSet)==20,
              "the hand-tracking structures are rebuilt in rust/crates/cvr-rig/src/vrik/fingergrip.rs");
static_assert(XR_TYPE_HAND_TRACKER_CREATE_INFO_EXT==1000051001 && XR_TYPE_HAND_JOINTS_LOCATE_INFO_EXT==1000051002 &&
              XR_TYPE_HAND_JOINT_LOCATIONS_EXT==1000051003 && XR_TYPE_HAND_JOINTS_MOTION_RANGE_INFO_EXT==1000080000 &&
              XR_HAND_LEFT_EXT==1 && XR_HAND_RIGHT_EXT==2 && XR_HAND_JOINT_SET_DEFAULT_EXT==0 &&
              XR_HAND_JOINTS_MOTION_RANGE_UNOBSTRUCTED_EXT==1 && XR_SPACE_LOCATION_POSITION_VALID_BIT==2 &&
              XR_HAND_JOINT_COUNT_EXT==26 && XR_HAND_JOINT_WRIST_EXT==1 && XR_HAND_JOINT_THUMB_METACARPAL_EXT==2 &&
              XR_HAND_JOINT_INDEX_METACARPAL_EXT==6 && XR_HAND_JOINT_MIDDLE_METACARPAL_EXT==11 &&
              XR_HAND_JOINT_RING_METACARPAL_EXT==16 && XR_HAND_JOINT_LITTLE_METACARPAL_EXT==21 &&
              XR_HAND_JOINT_LITTLE_TIP_EXT==25 && sizeof(XrInstance)==8 && sizeof(XrTime)==8,
              "the hand-tracking constants are repeated in rust/crates/cvr-rig/src/vrik/fingergrip.rs");
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
bool cvr::fingergrip::GripFromFingers() { return cvr_fingergrip_grip_from_fingers(); }
#endif
// The weapon hand's layers (src/Anim/LeftHanded.cpp, HeadAimWeapon.cpp, FkRecorder.cpp, AdsEyeAlign.cpp and
// AdsMuzzleStabilizer.cpp in Rust: rust/crates/cvr-rig/src/vrik/weapon_arm.rs).
extern "C" {
bool cvr_anim_is_head_aim_weapon_active();
#if !CVR_RUST_VRIK   // unused by the Rust build
bool cvr_anim_write_weapon_model_rot_via_hand(uint8_t* boneBuf,int weaponIdx,const float* desiredWeaponModel,
                                              const float* weaponLocalOverride);
#endif
}
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
bool cvr::anim::IsHeadAimWeaponActive() { return cvr_anim_is_head_aim_weapon_active(); }
#endif

// The smoking poses and the native reload's arms (src/Anim/SmokingPose.cpp and ReloadHandOff.cpp in Rust:
// rust/crates/cvr-rig/src/vrik/smoking.rs and reload_handoff.rs).
// The weapon's bones (src/Anim/WeaponRig.cpp in Rust: rust/crates/cvr-rig/src/vrik/weapon_rig.rs).
extern "C" {
#if !CVR_RUST_VRIK   // unused by the Rust build
void cvr_anim_weapon_rig_identify_and_write(void** a1,void** a2,unsigned int a4,void* poseDesc,uint8_t* boneBuf,
                                            uintptr_t trackBuf);
#endif
}
// Hands on the steering wheel (src/Anim/WheelGrab.cpp in Rust: rust/crates/cvr-rig/src/vrik/wheel_grab.rs).
static_assert(sizeof(cvr::anim::WheelTrackingFrame)==88 && offsetof(cvr::anim::WheelTrackingFrame,hands)==28 &&
              offsetof(cvr::anim::WheelTrackingFrame,sequence)==56 && offsetof(cvr::anim::WheelTrackingFrame,validHands)==80,
              "cvr::anim::WheelTrackingFrame is shared with rust/crates/cvr-rig/src/wheel.rs");
// The wheel's UI gate and tracking frame: rust/crates/cvr-render/src/rig_services.rs.

// The controllers' vibration (src/Runtimes/Haptics.cpp in Rust: rust/crates/cvr-rig/src/vrik/haptics.rs; the device call
// stays in src/Runtimes/HapticsDevice.cpp).

// The grasp (src/Anim/GraspPose.cpp in Rust: rust/crates/cvr-rig/src/vrik/grasp.rs; cvr_grasp_* are its own).

// The support hand on the weapon (src/Anim/TwoHandGrip.cpp in Rust: rust/crates/cvr-rig/src/vrik/two_hand.rs; the carry's
// C names are its own).
// The pose hook's work (src/Hooks/AnimPoseBody.cpp in Rust: rust/crates/cvr-rig/src/vrik/pose_hook.rs; cvr_anim_pose_apply
// is its own): the C++ it reads -- the player-state lock, the scene and vehicle state, the ladder, the lobby, full-body
// tracking, swimming, roomscale, the camera link, body follow and the pose-identity diagnostics. (The device camera,
// the ladder's hands, the roomscale camera, body follow and the tracked yaw: rust/crates/cvr-render/src/cpp_services.rs.)
static_assert(sizeof(cvr::camera::AnchorRecipe)==44,"cvr::camera::AnchorRecipe is shared with rust/crates/cvr-camera/src/anchor.rs");
extern "C" {
// The pose's heading-ready flag: rust/crates/cvr-render/src/rig_services.rs.
// The cutscene tier and the pose identity's diagnostics: rig_services.rs.
bool cvr_cpp_fbt_solve_arm(uint8_t* boneBuf,int side,const float* target,const float* rot) {
    return cvr::fbt::game::SolveArm(boneBuf,side,target,rot);
}
void cvr_cpp_fbt_solve_lower_body(uint8_t* boneBuf,const float* view,const float* rot) {
    cvr::fbt::game::SolveLowerBody(boneBuf,view,rot);
}
}

#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
bool cvr::camera::ReadNeckCameraMount(AnchorVector* offset) { return cvr_vrik_read_neck_camera_mount(offset); }
#endif

extern "C" {
// The frame's head orientation, the log, the snap-window trace and the grip and config paths: rig_services.rs.
bool cvr_cpp_vrik_fbt_body_active() { return cvr::fbt::game::BodyActive(); }
}
