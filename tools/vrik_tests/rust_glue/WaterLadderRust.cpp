// tools/vrik_tests' copy of src/Hooks/WaterLadderRust.cpp as the plugin had it when the plugin's copy started losing what only these
// programs call (the C forms of the C++ the Rust solve reads, and the C++ names over the solve's Rust exports).
// The Rust variants (vrik_*_tests_rust) compile this file in place of the plugin's.
// The ladder and the swim strokes in Rust (CVR_RUST_VRIK: rust/crates/cvr-rig/src/vrik/ladder_input.rs and
// swimming_input.rs) in place of src/Hooks/LadderInput.cpp and SwimmingInput.cpp: cvr::ladder::* and cvr::swimming::*,
// forwarded.
#include "Hooks/LadderInput.hpp"
#include "Hooks/SwimmingInput.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include <cstddef>

extern "C" {
bool cvr_ladder_active();
void cvr_ladder_read_hand_kinds(int* kinds);
void cvr_ladder_publish(uintptr_t player,int detailed,bool paused,const cvr::ladder::Geometry* geometry);
bool cvr_ladder_publish_top(uintptr_t player,const cvr::ladder::Vec* ladderPosition,const cvr::ladder::Vec* origin,
                            const cvr::ladder::Vec* right,const cvr::ladder::Vec* normal,const cvr::ladder::Vec* up);
bool cvr_ladder_publish_measured_top(uintptr_t player,const cvr::ladder::Vec* ladderPosition,float top);
void cvr_ladder_update_input(const VRControllerState* controllers);
void cvr_ladder_reset_input();
float cvr_ladder_mix_move(float original,float rawX,float rawY);
float cvr_ladder_native_action();
void cvr_ladder_constrain_hand(int side,float* targetModel,float* handRotation,const float* position,const float* rotation);
float cvr_ladder_debug_value(int index);
bool cvr_ladder_set_simulator_grip(float left,float right,int milliseconds);
bool cvr_swimming_active();
void cvr_swimming_bind_native_input(void* locomotion);
float cvr_swimming_native_action(int action);
bool cvr_swimming_read_steering(XrVector3f* direction,int* state);
void cvr_swimming_publish_state(uintptr_t player,int state,int fast,bool paused,bool waterContext);
void cvr_swimming_reset_input();
void cvr_swimming_update_input(const VRControllerState* controllers,bool manualBack,cvr::swimming::Input* out);
bool cvr_swimming_sprint_input(bool wanted);
float cvr_swimming_debug_value(int index);
float cvr_swimming_script_action(uintptr_t owner,uint64_t name,bool justPressed);
bool cvr_swimming_is_player(uintptr_t owner);
}
static_assert(sizeof(cvr::ladder::Vec)==12 && sizeof(cvr::ladder::Geometry::Segment)==24 && sizeof(cvr::ladder::Geometry)==336 &&
              offsetof(cvr::ladder::Geometry,height)==48 && offsetof(cvr::ladder::Geometry,topRails)==72 &&
              offsetof(cvr::ladder::Geometry,topRailCount)==312 && offsetof(cvr::ladder::Geometry,identity)==320 &&
              offsetof(cvr::ladder::Geometry,valid)==328,
              "cvr::ladder::Geometry is mirrored in rust/crates/cvr-rig/src/ladder_climb.rs");

namespace cvr::ladder {
bool Active() { return cvr_ladder_active(); }
void ReadHandKinds(int* kinds) { cvr_ladder_read_hand_kinds(kinds); }
void Publish(uintptr_t player,int detailed,bool paused,const Geometry* geometry) { cvr_ladder_publish(player,detailed,paused,geometry); }
bool PublishTop(uintptr_t player,Vec ladderPosition,Vec origin,Vec right,Vec normal,Vec up) {
    return cvr_ladder_publish_top(player,&ladderPosition,&origin,&right,&normal,&up);
}
bool PublishMeasuredTop(uintptr_t player,Vec ladderPosition,float top) { return cvr_ladder_publish_measured_top(player,&ladderPosition,top); }
void UpdateInput(const VRControllerState& controllers) { cvr_ladder_update_input(&controllers); }
void ResetInput() { cvr_ladder_reset_input(); }
float MixMove(float original,float rawX,float rawY) { return cvr_ladder_mix_move(original,rawX,rawY); }
float NativeAction() { return cvr_ladder_native_action(); }
void ConstrainHand(int side,float* targetModel,float* handRotation,const float* position,const float* rotation) {
    cvr_ladder_constrain_hand(side,targetModel,handRotation,position,rotation);
}
float DebugValue(int index) { return cvr_ladder_debug_value(index); }
bool SetSimulatorGrip(float left,float right,int milliseconds) { return cvr_ladder_set_simulator_grip(left,right,milliseconds); }
}
namespace cvr::swimming {
bool Active() { return cvr_swimming_active(); }
void BindNativeInput(void* locomotion) { cvr_swimming_bind_native_input(locomotion); }
float NativeAction(int action) { return cvr_swimming_native_action(action); }
bool ReadSteering(XrVector3f* direction,int* state) { return cvr_swimming_read_steering(direction,state); }
void PublishState(uintptr_t player,int state,int fast,bool paused,bool waterContext) { cvr_swimming_publish_state(player,state,fast,paused,waterContext); }
void ResetInput() { cvr_swimming_reset_input(); }
Input UpdateInput(const VRControllerState& controllers,bool manualBack) { Input out{};cvr_swimming_update_input(&controllers,manualBack,&out);return out; }
bool SprintInput(bool wanted) { return cvr_swimming_sprint_input(wanted); }
float DebugValue(int index) { return cvr_swimming_debug_value(index); }
float ScriptAction(uintptr_t owner,uint64_t name,bool justPressed) { return cvr_swimming_script_action(owner,name,justPressed); }
bool IsPlayer(uintptr_t owner) { return cvr_swimming_is_player(owner); }
}