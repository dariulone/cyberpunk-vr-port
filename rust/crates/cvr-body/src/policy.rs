//! Which gameplay states hand the body to the animation and who owns turning and the vehicle buttons (formerly
//! include/Anim/{ScenePolicy,VehiclePosePolicy}.hpp, include/Hooks/{RoomscaleEligibility,TurnInput,VehicleButtons}.hpp).
/// RED4ext::GameplayTier.
pub mod tier {
    pub const FULL_GAMEPLAY: i32 = 1;
    pub const CINEMATIC: i32 = 5;
}
/// RED4ext::game::PSMVehicle.
pub mod psm_vehicle {
    pub const DRIVING: i32 = 1;
    pub const COMBAT: i32 = 2;
    pub const PASSENGER: i32 = 3;
    pub const DRIVER_COMBAT: i32 = 6;
}

pub fn is_driving_vehicle_state(state: i32) -> bool {
    state == psm_vehicle::DRIVING || state == psm_vehicle::DRIVER_COMBAT
}
/// Driving or sitting as a passenger (main's VehiclePosePolicy IsSeatedVehicleState, cccb014f7).
pub fn is_seated_vehicle_state(state: i32) -> bool {
    is_driving_vehicle_state(state) || state == psm_vehicle::PASSENGER
}
pub fn is_passenger_window_combat(state: i32) -> bool {
    state == psm_vehicle::COMBAT
}

pub fn is_controller_vrik_enabled(binding_mode: i32, head_aim_active: bool) -> bool {
    binding_mode == 4 && !head_aim_active
}
/// The stored UI values: -1/0 disable, 1..4 select Tier2..Tier5 (the engine enum is one-based).
pub fn should_suspend_vrik(engine_tier: i32, configured_tier: i32) -> bool {
    if !(1..=4).contains(&configured_tier) {
        return false;
    }
    let minimum = tier::FULL_GAMEPLAY + configured_tier;
    engine_tier >= minimum && engine_tier <= tier::CINEMATIC
}
pub fn should_suspend_vrik_in(engine_tier: i32, in_vehicle: bool, on_foot_tier: i32, vehicle_tier: i32) -> bool {
    should_suspend_vrik(engine_tier, if in_vehicle { vehicle_tier } else { on_foot_tier })
}
/// A GRAPPLE OR TAKEDOWN IS A PAIRED ANIMATION (gamePSMTakedown 1..4).
pub fn should_suspend_vrik_for_takedown(takedown_state: i32, enabled: i32) -> bool {
    enabled != 0 && (1..=4).contains(&takedown_state)
}
/// A CARRIED BODY: lifting, dropping, throwing, disposing (and a container's workspot, locomotion 9).
pub fn should_suspend_vrik_for_body_carry(carrying: i32, disposal: i32, locomotion: i32, enabled: i32) -> bool {
    enabled != 0
        && (carrying == 1 || carrying == 3 || carrying == 4 || carrying == 6 || ((1..=3).contains(&disposal) && locomotion == 9))
}
/// A FINISHER's paired animation; a flag older than 20 s never ended and must not hold VRIK off.
pub fn should_suspend_vrik_for_finisher(finisher_active: i32, age_ms: u64, enabled: i32) -> bool {
    enabled != 0 && finisher_active > 0 && age_ms < 20000
}
pub fn should_hand_left_arm_to_body_carry(carrying: i32, enabled: i32) -> bool {
    enabled != 0 && (carrying == 2 || carrying == 5)
}
pub fn should_hand_right_arm_to_body_carry(carrying: i32, both_hands: i32, enabled: i32) -> bool {
    should_hand_left_arm_to_body_carry(carrying, enabled) && both_hands > 0
}
pub fn resolve_vehicle_vrik_suspend_tier(on_foot_tier: i32, vehicle_tier: i32, explicitly_configured: bool) -> i32 {
    let t = if explicitly_configured { vehicle_tier } else { on_foot_tier };
    t.clamp(-1, 4)
}

/// Follow the VRIK scene setting instead of a second Tier1-only gate.
pub fn is_vrik_pose_tier_allowed(tier_value: i32, suspend_tier: i32) -> bool {
    tier_value >= tier::FULL_GAMEPLAY && tier_value <= tier::CINEMATIC && !should_suspend_vrik(tier_value, suspend_tier)
}
pub fn is_vrik_body_movement_allowed(binding_mode: i32, head_aim_active: bool, in_vehicle: bool, tier_value: i32,
    suspend_tier: i32) -> bool {
    !in_vehicle && is_controller_vrik_enabled(binding_mode, head_aim_active) && is_vrik_pose_tier_allowed(tier_value, suspend_tier)
}

pub fn vehicle_owns_turn(mounted: bool, vehicle_state: i32) -> bool {
    mounted || is_passenger_window_combat(vehicle_state)
}
pub fn native_owns_turn(mounted: bool, vehicle_state: i32, on_ladder: bool) -> bool {
    on_ladder || vehicle_owns_turn(mounted, vehicle_state)
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TurnInput {
    pub axis: f32,
    pub snap_degrees: f32,
    pub clear_pending: bool,
}

/// The snap thresholds are raw stick travel, independent of the smooth-look curve (main's TurnInput.hpp: requiring
/// > 0.90 after the deadzone meant > 0.918 of raw travel and missed slightly diagonal flicks).
pub const SNAP_TURN_STICK_FIRE: f32 = 0.80;
pub const SNAP_TURN_STICK_REARM: f32 = 0.50;

#[allow(clippy::neg_cmp_op_on_partial_ord)] // a NaN setting falls to the default, as in C++
pub fn route_turn(axis: f32, snap_enabled: bool, armed_direction: &mut i32, angle: f32, fire: f32, rearm: f32) -> TurnInput {
    if !axis.is_finite() {
        return TurnInput { axis: 0.0, snap_degrees: 0.0, clear_pending: !snap_enabled };
    }
    let fire = if !(fire > 0.05) || fire > 1.0 { SNAP_TURN_STICK_FIRE } else { fire };
    let rearm = if !(rearm >= 0.0) || rearm >= fire { fire * 0.55 } else { rearm };
    if !snap_enabled {
        // Native vehicle look keeps the axis; a held stick must be released before it can snap afterwards.
        *armed_direction = if axis.abs() <= rearm { 0 } else if axis > 0.0 { 1 } else { -1 };
        return TurnInput { axis, snap_degrees: 0.0, clear_pending: true };
    }
    let direction = if axis >= fire { 1 } else if axis <= -fire { -1 } else { 0 };
    if axis.abs() <= rearm {
        *armed_direction = 0;
    }
    let mut delta = 0.0f32;
    if direction != 0 && direction != *armed_direction {
        *armed_direction = direction;
        delta = -(direction as f32) * if angle.is_finite() && angle > 0.0 { angle } else { 30.0 };
    }
    TurnInput { axis: 0.0, snap_degrees: delta, clear_pending: false }
}
/// The controller's turn (main's RouteControllerTurn): the snap on the raw travel, the smooth look on the curved axis.
pub fn route_controller_turn(raw_axis: f32, look_axis: f32, snap_enabled: bool, armed_direction: &mut i32, angle: f32, fire: f32, rearm: f32) -> TurnInput {
    let mut turn = route_turn(raw_axis, snap_enabled, armed_direction, angle, fire, rearm);
    if !snap_enabled {
        turn.axis = if look_axis.is_finite() { look_axis } else { 0.0 };
    }
    turn
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VehicleButtons {
    pub owned: u16,
    pub exit_on_x: bool,
    pub confirm_on_a: bool,
}

/// Window combat keeps vanilla X reload; B stays protected from an accidental exit.
pub fn vehicle_button_policy(mounted: bool, state: i32) -> VehicleButtons {
    if is_passenger_window_combat(state) {
        return VehicleButtons { owned: 0x2000, exit_on_x: false, confirm_on_a: false };
    }
    if mounted {
        VehicleButtons { owned: 0x6000, exit_on_x: true, confirm_on_a: true }
    } else {
        VehicleButtons::default()
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VehicleExitHold {
    since: u64,
    down: bool,
}

impl VehicleExitHold {
    pub fn update(&mut self, allowed: bool, x_held: bool, now: u64, hold_ms: u64) -> bool {
        if !allowed || !x_held {
            self.down = false;
            return false;
        }
        if !self.down {
            self.down = true;
            self.since = now;
        }
        now >= self.since && now - self.since >= hold_ms
    }
}
