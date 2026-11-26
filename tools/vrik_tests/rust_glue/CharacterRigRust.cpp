// tools/vrik_tests' copy of src/Anim/CharacterRigRust.cpp as the plugin had it when the plugin's copy started losing what only these
// programs call (the C forms of the C++ the Rust solve reads, and the C++ names over the solve's Rust exports).
// The Rust variants (vrik_*_tests_rust) compile this file in place of the plugin's.
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
extern "C" {
void cvr_anim_rest_finger_pose(uint8_t* boneBuf,bool force);
void cvr_anim_rest_finger_pose_right(uint8_t* boneBuf,bool force);
void cvr_anim_reload_finger_pose(uint8_t* boneBuf);
void cvr_anim_carry_grip_pose_right(uint8_t* boneBuf);
void cvr_anim_rest_finger_tick();
void cvr_anim_rest_finger_right_tick();
}
void cvr::anim::VrikRestFingerPose(uint8_t* boneBuf,bool force) { cvr_anim_rest_finger_pose(boneBuf,force); }
void cvr::anim::VrikRestFingerPoseRight(uint8_t* boneBuf,bool force) { cvr_anim_rest_finger_pose_right(boneBuf,force); }
void cvr::anim::VrikReloadFingerPose(uint8_t* boneBuf) { cvr_anim_reload_finger_pose(boneBuf); }
void cvr::anim::VrikCarryGripPoseRight(uint8_t* boneBuf) { cvr_anim_carry_grip_pose_right(boneBuf); }
void cvr::anim::RestFingerTick() { cvr_anim_rest_finger_tick(); }
void cvr::anim::RestFingerRightTick() { cvr_anim_rest_finger_right_tick(); }

// The finger gestures and the arm cyberware (src/Anim/HandGestures.cpp and Cyberware.cpp in Rust:
// rust/crates/cvr-rig/src/vrik/gestures.rs and cyberware.rs).
extern "C" {
void cvr_gestures_apply_calibration(int side,const char* suffix,const float* weights,float* rotation);
void cvr_gestures_publish_input(float leftGrip,float rightGrip,float leftTrigger,float rightTrigger,bool thumb,bool enabled);
void cvr_gestures_publish_equipment(int mask);
void cvr_gestures_apply(uint8_t* buffer);
#if !CVR_RUST_VRIK
void cvr_gestures_apply_gloves(uint8_t* buffer);
#endif
bool cvr_gestures_owns_hand(int side);
bool cvr_gestures_port_holds(int side);
bool cvr_cyberware_read_frame(cvr::cyberware::Frame* out);
bool cvr_cyberware_launcher_line(float* origin,float* direction);
float cvr_cyberware_wire_deployment(int side);
#if !CVR_RUST_VRIK
void cvr_cyberware_set_wire_planes(int side,uint32_t epoch,const cvr::cyberware::Plane* planes);
#endif
void cvr_cyberware_muscle_pulse(int side,float duration);
void cvr_cyberware_select(int kind);
int cvr_cyberware_selected_kind();
void cvr_cyberware_publish_grips(bool left,bool right);
void cvr_cyberware_collision_info(float* values);
}
static_assert(sizeof(cvr::cyberware::Frame)==1208 && sizeof(cvr::cyberware::Plane)==28,
              "cvr::cyberware::Frame and Plane are shared with rust/crates/cvr-rig/src/vrik/cyberware.rs");
void cvr::gestures::ApplyCalibration(int side,const char* suffix,const CalibrationWeights& weights,float* rotation) {
    cvr_gestures_apply_calibration(side,suffix,weights.data(),rotation);
}
void cvr::gestures::PublishInput(float lg,float rg,float lt,float rt,bool thumb,bool enabled) { cvr_gestures_publish_input(lg,rg,lt,rt,thumb,enabled); }
void cvr::gestures::PublishEquipment(int mask) { cvr_gestures_publish_equipment(mask); }
void cvr::gestures::Apply(uint8_t* buffer) { cvr_gestures_apply(buffer); }
#if !CVR_RUST_VRIK
void cvr::gestures::ApplyGloves(uint8_t* buffer) { cvr_gestures_apply_gloves(buffer); }
#endif
bool cvr::gestures::OwnsHand(int side) { return cvr_gestures_owns_hand(side); }
bool cvr::gestures::PortHolds(int side) { return cvr_gestures_port_holds(side); }
bool cvr::cyberware::ReadFrame(Frame& out) { return cvr_cyberware_read_frame(&out); }
bool cvr::cyberware::LauncherLine(float* origin,float* direction) { return cvr_cyberware_launcher_line(origin,direction); }
float cvr::cyberware::WireDeployment(int side) { return cvr_cyberware_wire_deployment(side); }
#if !CVR_RUST_VRIK
void cvr::cyberware::SetWirePlanes(int side,uint32_t epoch,const std::array<Plane,Wire::Count>& planes) {
    cvr_cyberware_set_wire_planes(side,epoch,planes.data());
}
#endif
void cvr::cyberware::MusclePulse(int side,float duration) { cvr_cyberware_muscle_pulse(side,duration); }
void cvr::cyberware::Select(int kind) { cvr_cyberware_select(kind); }
int cvr::cyberware::SelectedKind() { return cvr_cyberware_selected_kind(); }
void cvr::cyberware::PublishGrips(bool left,bool right) { cvr_cyberware_publish_grips(left,right); }
void cvr::cyberware::CollisionInfo(float* values) { cvr_cyberware_collision_info(values); }
// Finger tracking from the hand skeleton (src/Runtimes/FingerGrip.cpp in Rust: rust/crates/cvr-rig/src/vrik/fingergrip.rs).
// The extension list and the loader's xrGetInstanceProcAddr stay here; the structures Rust builds are openxr.h's.
extern "C" {
int cvr_fingergrip_choose_extensions(bool hasHandTracking,bool hasMotionRange);
void cvr_fingergrip_session_created(XrInstance instance,XrSession session,PFN_xrGetInstanceProcAddr getProc);
void cvr_fingergrip_session_destroying();
int cvr_fingergrip_evaluate(int hand,XrSpace base,XrTime time);
bool cvr_fingergrip_grip_from_fingers();
void cvr_fingergrip_set_grip_from_fingers(bool on);
bool cvr_fingergrip_tracked_curl(int hand,float* curl);
bool cvr_fingergrip_tracked_splay(int hand,float* splay);
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
void cvr::fingergrip::ChooseExtensions(const std::vector<XrExtensionProperties>& available,std::vector<const char*>& enabled) {
    bool handTracking=false,motionRange=false;
    for(const auto& p:available) {
        if(std::strcmp(p.extensionName,XR_EXT_HAND_TRACKING_EXTENSION_NAME)==0)handTracking=true;
        if(std::strcmp(p.extensionName,XR_EXT_HAND_JOINTS_MOTION_RANGE_EXTENSION_NAME)==0)motionRange=true;
    }
    const int mask=cvr_fingergrip_choose_extensions(handTracking,motionRange);
    if(mask&1)enabled.push_back(XR_EXT_HAND_TRACKING_EXTENSION_NAME);
    if(mask&2)enabled.push_back(XR_EXT_HAND_JOINTS_MOTION_RANGE_EXTENSION_NAME);
}
void cvr::fingergrip::OnSessionCreated(XrInstance instance,XrSession session) { cvr_fingergrip_session_created(instance,session,&xrGetInstanceProcAddr); }
void cvr::fingergrip::OnSessionDestroying() { cvr_fingergrip_session_destroying(); }
int cvr::fingergrip::Evaluate(int hand,XrSpace base,XrTime time) { return cvr_fingergrip_evaluate(hand,base,time); }
bool cvr::fingergrip::GripFromFingers() { return cvr_fingergrip_grip_from_fingers(); }
void cvr::fingergrip::SetGripFromFingers(bool on) { cvr_fingergrip_set_grip_from_fingers(on); }
bool cvr::fingergrip::TrackedCurl(int hand,float curl[5]) { return cvr_fingergrip_tracked_curl(hand,curl); }
bool cvr::fingergrip::TrackedSplay(int hand,float splay[4]) { return cvr_fingergrip_tracked_splay(hand,splay); }
// The weapon hand's layers (src/Anim/LeftHanded.cpp, HeadAimWeapon.cpp, FkRecorder.cpp, AdsEyeAlign.cpp and
// AdsMuzzleStabilizer.cpp in Rust: rust/crates/cvr-rig/src/vrik/weapon_arm.rs).
extern "C" {
bool cvr_anim_weapon_in_left_hand();
int cvr_anim_held_weapon_bone();
void cvr_anim_left_handed_arm_rig(const char* const* names,int count);
void cvr_anim_note_finger_flex(const uint8_t* boneBuf,bool drawn);
void cvr_anim_mirror_held_hand(float* pos,float* rot);
bool cvr_anim_mirror_finger_to_right(int leftBone,const float* rot,int* rightBone,float* out);
bool cvr_anim_mirror_weapon_slot_to_right(const float* pos,const float* rot,float* outPos,float* outRot);
bool cvr_anim_two_guns_in_hand();
bool cvr_anim_is_head_aim_weapon_active();
void cvr_anim_apply_head_aim_weapon_orientation(uint8_t* boneBuf);
void cvr_anim_publish_animated_fk(uint8_t* boneBuf);
bool cvr_anim_write_weapon_model_rot_via_hand(uint8_t* boneBuf,int weaponIdx,const float* desiredWeaponModel,
                                              const float* weaponLocalOverride);
void cvr_anim_prepare_aim_arm_targets(uint8_t* boneBuf);
void cvr_anim_solve_prepared_aim_arms(uint8_t* boneBuf);
void cvr_anim_apply_non_vrik_ads_muzzle_stabilizer(uint8_t* boneBuf);
}
bool cvr::anim::WeaponInLeftHand() { return cvr_anim_weapon_in_left_hand(); }
int cvr::anim::HeldWeaponBone() { return cvr_anim_held_weapon_bone(); }
void cvr::anim::LeftHandedArmRig(const char* const* names,int count) { cvr_anim_left_handed_arm_rig(names,count); }
void cvr::anim::NoteFingerFlex(const uint8_t* boneBuf,bool drawn) { cvr_anim_note_finger_flex(boneBuf,drawn); }
void cvr::anim::MirrorHeldHand(float* pos,float* rot) { cvr_anim_mirror_held_hand(pos,rot); }
bool cvr::anim::MirrorFingerToRight(int leftBone,const float* rot,int* rightBone,float* out) {
    return cvr_anim_mirror_finger_to_right(leftBone,rot,rightBone,out);
}
bool cvr::anim::MirrorWeaponSlotToRight(const float* pos,const float* rot,float* outPos,float* outRot) {
    return cvr_anim_mirror_weapon_slot_to_right(pos,rot,outPos,outRot);
}
bool cvr::anim::TwoGunsInHand() { return cvr_anim_two_guns_in_hand(); }
bool cvr::anim::IsHeadAimWeaponActive() { return cvr_anim_is_head_aim_weapon_active(); }
void cvr::anim::ApplyHeadAimWeaponOrientation(uint8_t* boneBuf) { cvr_anim_apply_head_aim_weapon_orientation(boneBuf); }
void cvr::anim::VrikPublishAnimatedFk(uint8_t* boneBuf) { cvr_anim_publish_animated_fk(boneBuf); }
#if !CVR_RUST_VRIK
bool cvr::anim::WriteWeaponModelRotViaHand(uint8_t* boneBuf,int weaponIdx,const float* desiredWeaponModel,
                                           const float* weaponLocalOverride) {
    return cvr_anim_write_weapon_model_rot_via_hand(boneBuf,weaponIdx,desiredWeaponModel,weaponLocalOverride);
}
#endif
void cvr::anim::PrepareAimArmTargets(uint8_t* boneBuf) { cvr_anim_prepare_aim_arm_targets(boneBuf); }
void cvr::anim::SolvePreparedAimArms(uint8_t* boneBuf) { cvr_anim_solve_prepared_aim_arms(boneBuf); }
void cvr::anim::ApplyNonVrikAdsMuzzleStabilizer(uint8_t* boneBuf) { cvr_anim_apply_non_vrik_ads_muzzle_stabilizer(boneBuf); }

// The smoking poses and the native reload's arms (src/Anim/SmokingPose.cpp and ReloadHandOff.cpp in Rust:
// rust/crates/cvr-rig/src/vrik/smoking.rs and reload_handoff.rs).
extern "C" {
void cvr_anim_smoking_cig_pose(uint8_t* boneBuf);
void cvr_anim_smoking_lighter_pose(uint8_t* boneBuf);
void cvr_anim_reload_hand_off_set(bool active,float finishInSec);
void cvr_anim_reload_hand_off_capture();
void cvr_anim_reload_hand_off_update();
float cvr_anim_reload_hand_off_weight();
void cvr_anim_reload_hand_off_blend_target(int hand,float* target,float* handRot);
}
void cvr::anim::VrikSmokingCigPose(uint8_t* boneBuf) { cvr_anim_smoking_cig_pose(boneBuf); }
void cvr::anim::VrikSmokingLighterPose(uint8_t* boneBuf) { cvr_anim_smoking_lighter_pose(boneBuf); }
void cvr::anim::ReloadHandOffSet(bool active,float finishInSec) { cvr_anim_reload_hand_off_set(active,finishInSec); }
void cvr::anim::ReloadHandOffCapture() { cvr_anim_reload_hand_off_capture(); }
void cvr::anim::ReloadHandOffUpdate() { cvr_anim_reload_hand_off_update(); }
float cvr::anim::ReloadHandOffWeight() { return cvr_anim_reload_hand_off_weight(); }
void cvr::anim::ReloadHandOffBlendTarget(int hand,float* target,float* handRot) {
    cvr_anim_reload_hand_off_blend_target(hand,target,handRot);
}
// The weapon's bones (src/Anim/WeaponRig.cpp in Rust: rust/crates/cvr-rig/src/vrik/weapon_rig.rs).
extern "C" {
void cvr_anim_weapon_rig_identify_and_write(void** a1,void** a2,unsigned int a4,void* poseDesc,uint8_t* boneBuf,
                                            uintptr_t trackBuf);
void cvr_anim_weapon_rig_census_note(unsigned int a4,uintptr_t trackBuf);
void cvr_anim_weapon_rig_capture_parts(uint8_t* boneBuf);
}
void cvr::anim::WeaponRigIdentifyAndWrite(void** a1,void** a2,unsigned int a4,void* poseDesc,uint8_t* boneBuf,
                                          uintptr_t trackBuf) {
    cvr_anim_weapon_rig_identify_and_write(a1,a2,a4,poseDesc,boneBuf,trackBuf);
}
void cvr::anim::WeaponRigCensusNote(unsigned int a4,uintptr_t trackBuf) { cvr_anim_weapon_rig_census_note(a4,trackBuf); }
void cvr::anim::WeaponRigCaptureParts(uint8_t* boneBuf) { cvr_anim_weapon_rig_capture_parts(boneBuf); }
// Hands on the steering wheel (src/Anim/WheelGrab.cpp in Rust: rust/crates/cvr-rig/src/vrik/wheel_grab.rs).
static_assert(sizeof(cvr::anim::WheelTrackingFrame)==88 && offsetof(cvr::anim::WheelTrackingFrame,hands)==28 &&
              offsetof(cvr::anim::WheelTrackingFrame,sequence)==56 && offsetof(cvr::anim::WheelTrackingFrame,validHands)==80,
              "cvr::anim::WheelTrackingFrame is shared with rust/crates/cvr-rig/src/wheel.rs");
extern "C" {
void cvr_anim_wheel_set_mounted_car(bool car);
void cvr_anim_wheel_capture_anim(int hand,int handIdx);
void cvr_anim_wheel_update(float dtSec);
void cvr_anim_wheel_publish_grab();
void cvr_anim_wheel_maintain_grab();
int cvr_anim_wheel_control_state();
float cvr_anim_wheel_steer_input();
bool cvr_anim_wheel_set_gamepad_profile(float inner,float outer);
float cvr_anim_wheel_gamepad_steer(float normalized);
void cvr_anim_wheel_blend_target(int hand,float* target,float* handRot);
void cvr_anim_wheel_store_target(int hand,const float* target);
bool cvr_anim_wheel_hands_off(int hand);
void cvr_anim_wheel_fingers(uint8_t* boneBuf);
void cvr_anim_wheel_reset();
// The steering settings of g_liveControls, as rust/crates/cvr-rig/src/vrik/wheel_grab.rs's WheelControls lays them out.
struct CvrWheelControls { int grab; float radius,steerMaxDeg,steerDeadDeg; int prediction; float predictionMs; int horn; float hornRadius; };
#if !CVR_RUST_VRIK   // the Rust build reads the live controls block itself (rust/crates/cvr-rig/src/vrik/live_reads.rs)
void cvr_cpp_wheel_controls(CvrWheelControls* out) {
    out->grab=g_liveControls.xrWheelGrab;out->radius=g_liveControls.xrWheelRadius;
    out->steerMaxDeg=g_liveControls.xrWheelSteerMaxDeg;out->steerDeadDeg=g_liveControls.xrWheelSteerDeadDeg;
    out->prediction=g_liveControls.xrWheelPrediction;out->predictionMs=g_liveControls.xrWheelPredictionMs;
    out->horn=g_liveControls.xrWheelHorn;out->hornRadius=g_liveControls.xrWheelHornRadius;
}
#endif
bool cvr_cpp_wheel_driving() { return g_isDriving.load(std::memory_order_relaxed); }
bool cvr_cpp_wheel_ui_blocks() { return g_menuModeValue!=0 || OverlayIsVisible(); }
// The head, then both hands head-relative, right then left (as WheelGrab.cpp's WheelTrackingSample).
bool cvr_cpp_wheel_tracking_frame(cvr::anim::WheelTrackingFrame* frame) {
    OpenXRHeadPose head{},hands[2]{};uint64_t sequence{},stamp{};
    if(!OpenXRManager::Get().GetGestureHandFrame(&head,hands,&sequence,&stamp))return false;
    frame->head={{head.oriX,head.oriY,head.oriZ,head.oriW},{head.posX,head.posY,head.posZ}};
    frame->sequence=sequence;frame->origin=head.originSerial;frame->stampUs=stamp;frame->headValid=head.valid;
    for(int i=0;i<2;++i) {
        const auto& hand=hands[1-i]; // tracking uses left/right, wheel uses right/left
        frame->hands[i]={hand.posX,hand.posY,hand.posZ};
        if(hand.valid)frame->validHands|=uint8_t(1u<<i);
    }
    return true;
}
}
void cvr::anim::WheelSetMountedCar(bool car) { cvr_anim_wheel_set_mounted_car(car); }
void cvr::anim::WheelCaptureAnim(int hand,int handIdx) { cvr_anim_wheel_capture_anim(hand,handIdx); }
void cvr::anim::WheelUpdate(float dtSec) { cvr_anim_wheel_update(dtSec); }
void cvr::anim::WheelPublishGrab() { cvr_anim_wheel_publish_grab(); }
void cvr::anim::WheelMaintainGrab() { cvr_anim_wheel_maintain_grab(); }
int cvr::anim::WheelControlState() { return cvr_anim_wheel_control_state(); }
float cvr::anim::WheelSteerInput() { return cvr_anim_wheel_steer_input(); }
bool cvr::anim::WheelSetGamepadProfile(float inner,float outer) { return cvr_anim_wheel_set_gamepad_profile(inner,outer); }
float cvr::anim::WheelGamepadSteer(float normalized) { return cvr_anim_wheel_gamepad_steer(normalized); }
void cvr::anim::WheelBlendTarget(int hand,float* target,float* handRot) { cvr_anim_wheel_blend_target(hand,target,handRot); }
void cvr::anim::WheelStoreTarget(int hand,const float* target) { cvr_anim_wheel_store_target(hand,target); }
bool cvr::anim::WheelHandsOff(int hand) { return cvr_anim_wheel_hands_off(hand); }
void cvr::anim::WheelFingers(uint8_t* boneBuf) { cvr_anim_wheel_fingers(boneBuf); }
void cvr::anim::WheelReset() { cvr_anim_wheel_reset(); }

// The controllers' vibration (src/Runtimes/Haptics.cpp in Rust: rust/crates/cvr-rig/src/vrik/haptics.rs; the device call
// stays in src/Runtimes/HapticsDevice.cpp).
extern "C" {
void cvr_haptics_shot(int weaponClass,bool supportHandOnGun,int hand);
void cvr_haptics_play(int event,int handMask);
void cvr_haptics_tick();
}
void cvr::haptics::Shot(int weaponClass,bool supportHandOnGun,int hand) { cvr_haptics_shot(weaponClass,supportHandOnGun,hand); }
void cvr::haptics::Play(int event,int handMask) { cvr_haptics_play(event,handMask); }
void cvr::haptics::Tick() { cvr_haptics_tick(); }

// The grasp (src/Anim/GraspPose.cpp in Rust: rust/crates/cvr-rig/src/vrik/grasp.rs; cvr_grasp_* are its own).
extern "C" {
void cvr_anim_grasp_fingers(uint8_t* boneBuf);
bool cvr_anim_grasp_holding(int side);
}
void cvr::anim::GraspFingers(uint8_t* boneBuf) { cvr_anim_grasp_fingers(boneBuf); }
bool cvr::anim::GraspHolding(int side) { return cvr_anim_grasp_holding(side); }

// The support hand on the weapon (src/Anim/TwoHandGrip.cpp in Rust: rust/crates/cvr-rig/src/vrik/two_hand.rs; the carry's
// C names are its own).
extern "C" {
void cvr_anim_two_hand_capture(uint8_t* boneBuf);
void cvr_anim_two_hand_right(const float* targetR,float* hm,const float* wristR,const float* leftCtrlModel);
bool cvr_anim_two_hand_left(float* target,float* handRot);
#if !CVR_RUST_VRIK
void cvr_anim_two_hand_select_hold_for_carry();
#endif
void cvr_anim_two_hand_fingers(uint8_t* boneBuf);
void cvr_anim_two_hand_tick();
}
void cvr::anim::TwoHandCapture(uint8_t* boneBuf) { cvr_anim_two_hand_capture(boneBuf); }
void cvr::anim::TwoHandRight(const float* targetR,float* hm,const float* wristR,const float* leftCtrlModel) {
    cvr_anim_two_hand_right(targetR,hm,wristR,leftCtrlModel);
}
bool cvr::anim::TwoHandLeft(float* target,float* handRot) { return cvr_anim_two_hand_left(target,handRot); }
#if !CVR_RUST_VRIK
void cvr::anim::SelectHoldForCarry() { cvr_anim_two_hand_select_hold_for_carry(); }
#endif
void cvr::anim::TwoHandFingers(uint8_t* boneBuf) { cvr_anim_two_hand_fingers(boneBuf); }
void cvr::anim::TwoHandTick() { cvr_anim_two_hand_tick(); }
// The pose hook's work (src/Hooks/AnimPoseBody.cpp in Rust: rust/crates/cvr-rig/src/vrik/pose_hook.rs; cvr_anim_pose_apply
// is its own): the C++ it reads -- the player-state lock, the scene and vehicle state, the ladder, the lobby, full-body
// tracking, swimming, roomscale, the camera link, body follow and the pose-identity diagnostics. (The device camera,
// the ladder's hands, the roomscale camera, body follow and the tracked yaw: rust/crates/cvr-render/src/cpp_services.rs.)
static_assert(sizeof(cvr::camera::AnchorRecipe)==44,"cvr::camera::AnchorRecipe is shared with rust/crates/cvr-camera/src/anchor.rs");
extern "C" {
void cvr_cpp_pose_lock() { AcquireSRWLockExclusive(&g_PlayerPoseStateLock); }
void cvr_cpp_pose_unlock() { ReleaseSRWLockExclusive(&g_PlayerPoseStateLock); }
void cvr_cpp_set_pose_heading_ready(bool ready) { g_PlayerPoseHeadingReady.store(ready,std::memory_order_release); }
int cvr_cpp_pose_scene_tier() { return g_sceneTier.load(std::memory_order_relaxed); }
int cvr_cpp_pose_vehicle_state() { return g_vehicleState.load(std::memory_order_relaxed); }
#if !CVR_RUST_VRIK   // the Rust build reads the live controls block itself (rust/crates/cvr-rig/src/vrik/live_reads.rs)
void cvr_cpp_pose_controls(int* out) {
    out[0]=g_liveControls.xrVehicleCutsceneSuspendTier;out[1]=g_liveControls.xrLadderGripClimb;out[2]=g_liveControls.xrLeftHanded;
}
#endif
int cvr_cpp_pose_effective_cutscene_tier() { return VRIK_EffectiveCutsceneSuspendTier(); }
void cvr_cpp_pose_id_note(bool sameInput,uint64_t poseId) {
    CVR_DIAGNOSTIC(CyberpunkVR_PoseIdVrikSource.store(sameInput ? poseId : 0,std::memory_order_relaxed));
    if(!sameInput)CVR_DIAGNOSTIC(CyberpunkVR_PoseIdVrikInputChanged.fetch_add(1,std::memory_order_relaxed));
}
bool cvr_cpp_ladder_active() { return cvr::ladder::Active(); }
bool cvr_cpp_fbt_calibrating() { return cvr::fbt::game::Calibrating(); }
bool cvr_cpp_fbt_solve_arm(uint8_t* boneBuf,int side,const float* target,const float* rot) {
    return cvr::fbt::game::SolveArm(boneBuf,side,target,rot);
}
void cvr_cpp_fbt_solve_lower_body(uint8_t* boneBuf,const float* view,const float* rot) {
    cvr::fbt::game::SolveLowerBody(boneBuf,view,rot);
}
bool cvr_cpp_swimming_active() { return cvr::swimming::Active(); }
}
extern uintptr_t g_gameModuleBase;   // src/Core/VrCore.cpp
extern size_t    g_gameModuleSize;
extern "C" void cvr_cpp_game_module_range(uintptr_t* base,size_t* size) { *base=g_gameModuleBase;*size=g_gameModuleSize; }

bool cvr::camera::ReadNeckCameraMount(AnchorVector* offset) { return cvr_vrik_read_neck_camera_mount(offset); }
bool cvr::camera::ReadNeckCameraEyeOffset(AnchorVector* offset) { return cvr_vrik_read_neck_camera_eye_offset(offset); }
bool cvr::gestures::ThumbReference(int side,float* rotation) { return cvr_vrik_thumb_reference(side,rotation); }
bool cvr::gestures::SpreadAxis(int side,int digit,int* bone,float axis[3]) { return cvr_vrik_spread_axis(side,digit,bone,axis); }

extern "C" {
// The head orientation PatchCamera composes with in this aim epoch (x y z w, OpenXR axes); false without one.
bool cvr_cpp_vrik_frame_head_orientation(float* out) {
    OpenXRHeadPose head{};
    if(!OpenXRManager::Get().AcquireFrameHeadSample(&head) || !head.valid)return false;
    out[0]=head.oriX;out[1]=head.oriY;out[2]=head.oriZ;out[3]=head.oriW;
    return true;
}
bool cvr_cpp_vrik_fbt_body_active() { return cvr::fbt::game::BodyActive(); }
void cvr_cpp_vrik_log(const char* text) { Log("%s",text); }
// One line of the snap-window trace, log\cyberpunkvr_snapwin.log (truncated per session, flushed per line), as
// HandSnapshot.cpp's VRIK_SnapTraceLog wrote it.
void cvr_cpp_vrik_snap_trace(const char* line) {
    static FILE* s_tf=nullptr;
    if(!s_tf)s_tf=_wfsopen(cvr::paths::File(cvr::paths::Folder::Log,L"cyberpunkvr_snapwin.log").c_str(),L"w",_SH_DENYNO);
    if(!s_tf || !line)return;
    std::fputs(line,s_tf);std::fflush(s_tf);
}
// A recorded grip's file (VRGripPath), NUL-terminated into out; false when it has none.
bool cvr_cpp_vrik_grip_path(const char* name,char* out,size_t size) {
    if(!out || !size)return false;
    const std::string path=VRGripPath(name);
    if(path.empty() || path.size()+1>size) { out[0]=0;return false; }
    std::memcpy(out,path.c_str(),path.size()+1);
    return true;
}
// The plugin's config folder's file `name` (cvr::paths::FileA), NUL-terminated into out; false when it has none.
bool cvr_cpp_vrik_config_file(const char* name,char* out,size_t size) {
    if(!out || !size)return false;
    const std::string path=cvr::paths::FileA(cvr::paths::Folder::Config,name);
    if(path.empty() || path.size()+1>size) { out[0]=0;return false; }
    std::memcpy(out,path.c_str(),path.size()+1);
    return true;
}
}
