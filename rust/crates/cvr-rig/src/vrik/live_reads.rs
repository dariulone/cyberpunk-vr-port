//! The live controls the solve reads, straight from the C++ half's block (cvr-settings' live_controls_block, attached
//! where `g_liveControls` is defined; formerly the cvr_cpp_*_controls adapters of src/Hooks/HookServices.cpp,
//! src/Hooks/InputServices.cpp and src/Anim/CharacterRigRust.cpp, which copied the same fields).

use super::ladder_input::LadderControls;
use super::roomscale_move::BodyControls;
use super::wheel_grab::WheelControls;
use super::xinput::InputControls;
use cvr_settings::lc;

/// xrSceneCamComfortDeg.
pub unsafe fn scene_cam_comfort_deg() -> f32 {
    lc!(xrSceneCamComfortDeg)
}
/// The head offsets on foot and in a vehicle (six floats); on a motorcycle its own trio replaces the car's (a rider
/// does not sit where a driver does).
pub unsafe fn camera_offsets(out: *mut f32) {
    let bike = super::wheel_grab::cvr_anim_wheel_mounted_bike();
    let values = if bike {
        [lc!(xrHeadOffsetX), lc!(xrHeadOffsetY), lc!(xrHeadOffsetZ), lc!(xrBikeHeadOffsetX), lc!(xrBikeHeadOffsetY), lc!(xrBikeHeadOffsetZ)]
    } else {
        [lc!(xrHeadOffsetX), lc!(xrHeadOffsetY), lc!(xrHeadOffsetZ), lc!(xrVehHeadOffsetX), lc!(xrVehHeadOffsetY), lc!(xrVehHeadOffsetZ)]
    };
    // SAFETY: the caller's six floats.
    unsafe { core::ptr::copy_nonoverlapping(values.as_ptr(), out, values.len()) };
}
/// The body's controls.
pub unsafe fn body_controls(out: *mut BodyControls) {
    // SAFETY: the caller's output.
    let Some(o) = (unsafe { out.as_mut() }) else { return };
    o.roomscale_movement = lc!(xrRoomscaleMovement);
    o.cutscene_suspend_tier = lc!(xrCutsceneSuspendTier);
    o.body_rotation_mode = lc!(xrBodyRotationMode);
    o.body_move_radius = lc!(xrBodyMoveRadius);
    o.body_free_look_deg = lc!(xrBodyFreeLookDeg);
    o.body_free_look_down_deg = lc!(xrBodyFreeLookDownDeg);
    o.body_free_look_swim_deg = lc!(xrBodyFreeLookSwimDeg);
}
/// xrMovementSource.
pub unsafe fn movement_source() -> i32 {
    lc!(xrMovementSource)
}
/// The ladder's controls.
pub unsafe fn ladder_controls(out: *mut LadderControls) {
    // SAFETY: the caller's output.
    let Some(o) = (unsafe { out.as_mut() }) else { return };
    o.grip_climb = lc!(xrLadderGripClimb);
    o.auto_finish = lc!(xrLadderAutoFinish);
    o.finish_distance = lc!(xrLadderFinishDistance);
}
/// xrBreaststrokeSwim.
pub unsafe fn breaststroke_swim() -> i32 {
    lc!(xrBreaststrokeSwim)
}
/// The input's controls.
pub unsafe fn input_controls(out: *mut InputControls) {
    // SAFETY: the caller's output.
    let Some(o) = (unsafe { out.as_mut() }) else { return };
    o.xinput_hook = lc!(xrXInputHook);
    o.left_handed = lc!(xrLeftHanded);
    o.left_handed_bindings = lc!(xrLeftHandedBindings);
    o.ladder_grip_climb = lc!(xrLadderGripClimb);
    o.movement_speed_mode = lc!(xrMovementSpeedMode);
    o.sprint_mode = lc!(xrSprintMode);
    o.vehicle_gun_trigger = lc!(xrVehicleGunTrigger);
    o.wheel_horn = lc!(xrWheelHorn);
    o.disable_mouse_y = lc!(xrDisableMouseY);
    o.snap_turn = lc!(xrSnapTurn);
    o.left_stick_deadzone = lc!(xrLeftStickDeadzone);
    o.right_stick_deadzone = lc!(xrRightStickDeadzone);
    o.max_input_threshold = lc!(xrMaxInputThreshold);
    o.vehicle_throttle_trim = lc!(xrVehicleThrottleTrim);
    o.snap_turn_angle_deg = lc!(xrSnapTurnAngleDeg);
}
/// The wheel's controls.
pub unsafe fn wheel_controls(out: *mut WheelControls) {
    // SAFETY: the caller's output.
    let Some(o) = (unsafe { out.as_mut() }) else { return };
    o.grab = lc!(xrWheelGrab);
    o.radius = lc!(xrWheelRadius);
    o.steer_max_deg = lc!(xrWheelSteerMaxDeg);
    o.steer_dead_deg = lc!(xrWheelSteerDeadDeg);
    o.prediction = lc!(xrWheelPrediction);
    o.prediction_ms = lc!(xrWheelPredictionMs);
    o.horn = lc!(xrWheelHorn);
    o.horn_radius = lc!(xrWheelHornRadius);
}
/// The pose hook's controls: the vehicle cutscene tier, the ladder's grip climb, the left-handed switch.
pub unsafe fn pose_controls(out: *mut i32) {
    let values = [lc!(xrVehicleCutsceneSuspendTier), lc!(xrLadderGripClimb), lc!(xrLeftHanded)];
    // SAFETY: the caller's three ints.
    unsafe { core::ptr::copy_nonoverlapping(values.as_ptr(), out, values.len()) };
}
