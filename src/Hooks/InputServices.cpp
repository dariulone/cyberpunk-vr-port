// What the XInput merge (rust/crates/cvr-rig/src/vrik/xinput.rs) needs from the rest of the plugin, in C form: OpenXR,
// the live controls, and the subsystems the merge feeds and asks (the quest clue, swimming, the ladder, the VR menu
// and overlay, the keypad, the lobby, the reach, the cyberware chord). Both builds.
#include "Hooks/CyberwareChord.hpp"
#include "Hooks/KeypadInput.hpp"
#include "Hooks/LadderInput.hpp"
#include "Hooks/SwimmingInput.hpp"
#include "Camera/CameraState.hpp"
#include "Core/LiveControls.hpp"
#include "Core/VrCoreShared.hpp"
#include "Lobby/Intro.hpp"
#include "Lobby/MenuLobby.hpp"
#include "Natives/Reach.hpp"
#include "Natives/VrMenu.hpp"
#include "Overlay/VrOverlay.hpp"
#include "Quest/StoryAttention.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include "Hooks/SwapChainInternal.hpp"
#include <atomic>
#include <cstddef>

// The cyberware chord's sent keys (src/Hooks/CyberwareChord.cpp, or rust/crates/cvr-rig/src/vrik/cyberware_chord.rs).
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/render_moved_exports.rs.
#else
extern "C" __declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_CyberwareChordSent{};
#endif

extern "C" {
// The g_liveControls fields the merge reads (xinput.rs InputControls, field for field).
#if !CVR_RUST_VRIK   // unused by the Rust build
struct CvrInputControls {
    int32_t xinputHook, leftHanded, leftHandedBindings, ladderGripClimb, movementSpeedMode, sprintMode, vehicleGunTrigger,
        wheelHorn, disableMouseY, snapTurn;
    float leftStickDeadzone, rightStickDeadzone, maxInputThreshold, vehicleThrottleTrim, snapTurnAngleDeg;
};
#endif
#if !CVR_RUST_VRIK   // unused by the Rust build
static_assert(sizeof(CvrInputControls) == 60, "mirrored by InputControls in rust/crates/cvr-rig/src/vrik/xinput.rs");
#endif
static_assert(sizeof(VRControllerState) == 80 && offsetof(VRControllerState, leftTrigger) == 4 &&
              offsetof(VRControllerState, fingerPull) == 16 && offsetof(VRControllerState, leftHandValid) == 56 &&
              offsetof(VRControllerState, psvr2DpadShift) == 60 && offsetof(VRControllerState, scannerPresses) == 64 &&
              offsetof(VRControllerState, scannerHeld) == 72,
              "VRControllerState is mirrored by VrController in rust/crates/cvr-rig/src/vrik/xinput.rs");
static_assert(sizeof(cvr::swimming::Input) == 16 && offsetof(cvr::swimming::Input, water) == 4,
              "cvr::swimming::Input is mirrored by SwimInput in rust/crates/cvr-rig/src/vrik/xinput.rs");
#if !CVR_RUST_VRIK   // the Rust build reads the live controls block itself (rust/crates/cvr-rig/src/vrik/live_reads.rs)
void cvr_cpp_input_controls(CvrInputControls* out) {
    const auto& c = g_liveControls;
    out->xinputHook = c.xrXInputHook; out->leftHanded = c.xrLeftHanded; out->leftHandedBindings = c.xrLeftHandedBindings;
    out->ladderGripClimb = c.xrLadderGripClimb; out->movementSpeedMode = c.xrMovementSpeedMode; out->sprintMode = c.xrSprintMode;
    out->vehicleGunTrigger = c.xrVehicleGunTrigger; out->wheelHorn = c.xrWheelHorn; out->disableMouseY = c.xrDisableMouseY;
    out->snapTurn = c.xrSnapTurn; out->leftStickDeadzone = c.xrLeftStickDeadzone; out->rightStickDeadzone = c.xrRightStickDeadzone;
    out->maxInputThreshold = c.xrMaxInputThreshold; out->vehicleThrottleTrim = c.xrVehicleThrottleTrim;
    out->snapTurnAngleDeg = c.xrSnapTurnAngleDeg;
}
#endif
#if !CVR_RUST_VRIK   // the Rust build calls the XR runtime's Rust function itself (rust/crates/cvr-rig/src/vrik/xr_calls.rs)
bool cvr_cpp_xr_controller_state(VRControllerState* out) { return OpenXRManager::Get().GetControllerState(out); }
#endif
#if !CVR_RUST_VRIK   // the Rust build's, and the forwards below: rust/crates/cvr-render/src/cpp_services.rs
void cvr_cpp_xr_set_shared_slot(int slot, float value) { OpenXRManager::Get().SetSharedSlot(slot, value); }
float cvr_cpp_xr_get_shared_slot(int slot) { return OpenXRManager::Get().GetSharedSlot(slot); }
#endif
#if !CVR_RUST_VRIK   // the Rust build calls the XR runtime's Rust function itself (rust/crates/cvr-rig/src/vrik/xr_calls.rs)
bool cvr_cpp_xr_gesture_hand_frame(OpenXRHeadPose* head, OpenXRHeadPose* hands, uint64_t* sequence, uint64_t* stamp, bool raw) {
    return OpenXRManager::Get().GetGestureHandFrame(head, hands, sequence, stamp, raw);
}
#endif
#if !CVR_RUST_VRIK   // the Rust build calls the XR runtime's Rust function itself (rust/crates/cvr-rig/src/vrik/xr_calls.rs)
bool cvr_cpp_xr_physical_crouch_requested() { return OpenXRManager::Get().PhysicalCrouchRequested(); }
#endif
#if !CVR_RUST_VRIK   // the Rust build's is rust/crates/cvr-render/src/cpp_services.rs
void cvr_cpp_quest_suspend_manual_clue_input() { cvr::quest::SuspendManualClueInput(); }
bool cvr_cpp_quest_consume_manual_clue_button(bool pressed, bool allowed) { return cvr::quest::ConsumeManualClueButton(pressed, allowed); }
void cvr_cpp_swimming_reset_input() { cvr::swimming::ResetInput(); }
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the Rust build's is rust/crates/cvr-render/src/cpp_services.rs
void cvr_cpp_swimming_update_input(const VRControllerState* c, bool manualBack, cvr::swimming::Input* out) {
    *out = cvr::swimming::UpdateInput(*c, manualBack);
}
#endif
#if !CVR_RUST_VRIK   // the Rust build's is rust/crates/cvr-render/src/cpp_services.rs
bool cvr_cpp_swimming_sprint_input(bool wanted) { return cvr::swimming::SprintInput(wanted); }
void cvr_cpp_ladder_reset_input() { cvr::ladder::ResetInput(); }
#endif
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/cpp_services.rs
void cvr_cpp_ladder_update_input(const VRControllerState* c) { cvr::ladder::UpdateInput(*c); }
#endif
#if !CVR_RUST_VRIK   // the Rust build's is rust/crates/cvr-render/src/cpp_services.rs
bool cvr_cpp_vrui_captures_input() { return cvr::vrui::CapturesInput(); }
bool cvr_cpp_reach_use_held() { return cvr::reach::UseHeld(); }
#endif
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/cpp_services.rs
void cvr_cpp_cyberware_chord_dispatch() { cvr::input::DispatchCyberwareChord(); }
#endif
#if !CVR_RUST_VRIK   // the Rust build's is rust/crates/cvr-render/src/cpp_services.rs
bool cvr_cpp_vrmenu_captures_buttons() { return cvr::vrmenu::CapturesButtons(); }
bool cvr_cpp_vrmenu_wheel_armed(uint64_t now) { return cvr::vrmenu::WheelArmed(now); }
void cvr_cpp_vrmenu_publish_wheel_hold(bool pending, bool held) { cvr::vrmenu::PublishWheelHold(pending, held); }
bool cvr_cpp_keypad_input_active() { return cvr::input::KeypadInputActive(); }
void cvr_cpp_keypad_publish_stick(float x, float y, float deadzone) { cvr::input::PublishKeypadStick(x, y, deadzone); }
float cvr_cpp_lobby_curtain() { return cvr::lobby::Curtain(); }
bool cvr_cpp_lobby_intro_running() { return cvr::lobby::intro::Running(); }
bool cvr_cpp_in_braindance() { return InBraindance(); }
bool cvr_cpp_scene_stick_yaw_armed() { return SceneStickYawArmed(); }
#endif
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/cpp_services.rs
void cvr_cpp_set_pending_snap_yaw(int32_t bits) { InterlockedExchange(&g_pendingSnapYawDeltaBits, bits); }
#endif
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/cpp_services.rs
int cvr_cpp_ui_popup_open() { return g_uiPopupOpen.load(std::memory_order_relaxed); }
#endif
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/cpp_services.rs
uint64_t cvr_cpp_ui_popup_closed_ms() { return g_uiPopupClosedMs.load(std::memory_order_relaxed); }
#endif
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/cpp_services.rs
void cvr_cpp_set_look_stick_x(float x) { g_lookStickX.store(x, std::memory_order_relaxed); }
#endif
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/cpp_services.rs
int cvr_cpp_verbose_log() { return g_verboseLog; }
#endif
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/cpp_services.rs
bool cvr_cpp_is_driving() { return g_isDriving.load(std::memory_order_relaxed); }
#endif
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/cpp_services.rs
bool cvr_cpp_game_window_in_front() { return GetForegroundWindow() == g_gameHwnd; }
#endif
}