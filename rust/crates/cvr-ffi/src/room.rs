//! Roomscale, the crouch, the native heading, hand reacquisition, the view's anchor and frames, and the scene and
//! input policies (stage 3: rust/crates/cvr-body, rust/crates/cvr-camera), for the C++ half. Objects are the C++
//! classes themselves (their layouts are Rust's); with CVR_RUST_ROOM their headers call these.
//!
//! # Safety
//! Every pointer is a live, properly aligned object of the named C++ type, used by one thread at a time.
#![allow(clippy::missing_safety_doc)]
use cvr_body::bend::BendSample;
use cvr_body::crouch::{self, PhysicalCrouch};
use cvr_body::native_heading::NativeBodyHeading;
use cvr_body::policy;
use cvr_body::reacquire::{self, HandPose, Reacquire};
use cvr_body::roomscale::{self, BodyTranslationFollow, LeadLimit, Movement, PhysicalWalkGate, Sample, Step};
use cvr_body::tracking;
use cvr_body::vec2::Vec2;
use cvr_body::xr::Quat;
use cvr_camera::anchor::{self, AnchorRecipe, AnchorVector};
use cvr_camera::eye_centre::EyeCentreLedger;
use cvr_camera::frames::{self, CameraEntitySample, HandViewFrame};
use cvr_camera::vehicle::{Selection, VehicleStereoHeading};

// ---- RoomscaleMovement ---------------------------------------------------------------------------------------------
#[no_mangle]
pub extern "C" fn cvr_room_rotate(v: Vec2, yaw: f32) -> Vec2 {
    cvr_body::vec2::rotate(v, yaw)
}
#[no_mangle]
pub extern "C" fn cvr_room_tracking_yaw(entity_yaw: f32, body_realign: f32) -> f32 {
    roomscale::tracking_yaw(entity_yaw, body_realign)
}
#[no_mangle]
pub unsafe extern "C" fn cvr_room_follow_reset(f: *mut BodyTranslationFollow) {
    unsafe { (*f).reset() }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_room_follow_head_motion(f: *mut BodyTranslationFollow, neck: Vec2, upright: bool, dt: f32) {
    unsafe { (*f).head_motion(neck, upright, dt) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_room_follow_head_still(f: *const BodyTranslationFollow) -> bool {
    unsafe { (*f).head_still() }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_room_follow_block(f: *mut BodyTranslationFollow, refused: Vec2) {
    unsafe { (*f).block(refused) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_room_follow_step(f: *mut BodyTranslationFollow, error: Vec2, radius: f32, dt: f32) -> Vec2 {
    unsafe { (*f).step(error, radius, dt) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_room_walk_gate_reset(g: *mut PhysicalWalkGate) {
    unsafe { (*g).reset() }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_room_walk_gate_update(g: *mut PhysicalWalkGate, velocity: Vec2, dt: f32) {
    unsafe { (*g).update(velocity, dt) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_room_walk_gate_reported(g: *const PhysicalWalkGate) -> Vec2 {
    unsafe { (*g).reported() }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_room_head_yaw(x: f32, y: f32, z: f32, w: f32, yaw: *mut f32) -> bool {
    match roomscale::head_yaw(x, y, z, w) {
        Some(v) if !yaw.is_null() => {
            unsafe { *yaw = v };
            true
        }
        _ => false,
    }
}
#[no_mangle]
pub extern "C" fn cvr_room_eye_from_neck(x: f32, y: f32, z: f32, w: f32) -> Vec2 {
    roomscale::eye_from_neck(x, y, z, w)
}
#[no_mangle]
pub unsafe extern "C" fn cvr_room_movement_frame_sample(tracking: *const Sample, frame: *const Sample, out: *mut Sample) {
    unsafe { *out = roomscale::movement_frame_sample(&*tracking, &*frame) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_room_movement_reset(m: *mut Movement) {
    unsafe { (*m).reset() }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_room_movement_suspend(m: *mut Movement) {
    unsafe { (*m).suspend() }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_room_movement_begin(m: *mut Movement, sample: *const Sample, now_us: u64, dt: f32, scale: f32,
    tracking_yaw: f32, allowed: bool, free_radius: f32, strict: bool, out: *mut Step) {
    unsafe { *out = (*m).begin(&*sample, now_us, dt, scale, tracking_yaw, allowed, free_radius, strict) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_room_movement_complete(m: *mut Movement, step: *const Step, accepted_world: Vec2,
    limit: *const LeadLimit) {
    unsafe { (*m).complete(&*step, accepted_world, *limit) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_room_movement_rest_neck(m: *const Movement, origin: u64, out: *mut Vec2) -> bool {
    match unsafe { (*m).rest_neck(origin) } {
        Some(v) if !out.is_null() => {
            unsafe { *out = v };
            true
        }
        _ => false,
    }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_room_movement_following(m: *const Movement) -> bool {
    unsafe { (*m).following() }
}
#[no_mangle]
pub extern "C" fn cvr_room_accepted_displacement(native: Vec2, physical: Vec2, resolved: Vec2) -> Vec2 {
    roomscale::accepted_displacement(native, physical, resolved)
}
#[no_mangle]
pub extern "C" fn cvr_room_physical_velocity(native: Vec2, physical: Vec2, resolved_velocity: Vec2, dt: f32) -> Vec2 {
    roomscale::physical_velocity(native, physical, resolved_velocity, dt)
}

// ---- PhysicalCrouch, NativeBodyHeading, TrackingReacquire, TrackingFilter ----------------------------------------------
#[no_mangle]
pub extern "C" fn cvr_crouch_threshold(value: f32) -> f32 {
    crouch::crouch_threshold(value)
}
#[no_mangle]
pub unsafe extern "C" fn cvr_crouch_depth(neck_drop: f32, bend: *const BendSample) -> f32 {
    unsafe { crouch::crouch_depth(neck_drop, &*bend) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_crouch_reset(c: *mut PhysicalCrouch) {
    unsafe { (*c).reset() }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_crouch_update(c: *mut PhysicalCrouch, depth: f32, threshold: f32, now: u64, origin: u64,
    owner: u64, allowed: bool) -> bool {
    unsafe { (*c).update(depth, threshold, now, origin, owner, allowed) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_native_heading_reset(h: *mut NativeBodyHeading) {
    unsafe { (*h).reset() }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_native_heading_matches(h: *const NativeBodyHeading, owner: usize, origin: u64) -> bool {
    unsafe { (*h).matches(owner, origin) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_native_heading_owned_by(h: *const NativeBodyHeading, owner: usize) -> bool {
    unsafe { (*h).owned_by(owner) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_native_heading_observe(h: *mut NativeBodyHeading, owner: usize, origin: u64, yaw: f32) -> bool {
    unsafe { (*h).observe(owner, origin, yaw) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_native_heading_commit(h: *mut NativeBodyHeading, yaw: f32, turn: f32) {
    unsafe { (*h).commit(yaw, turn) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_native_heading_offset(h: *const NativeBodyHeading) -> f32 {
    unsafe { (*h).offset() }
}
#[no_mangle]
pub extern "C" fn cvr_native_heading_wrap(yaw: f32) -> f32 {
    cvr_body::native_heading::wrap(yaw)
}
#[no_mangle]
pub unsafe extern "C" fn cvr_smooth_damp(current: f32, target: f32, velocity: *mut f32, smooth_time: f32, dt: f32) -> f32 {
    unsafe { reacquire::smooth_damp(current, target, &mut *velocity, smooth_time, dt) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_reacquire_reset(r: *mut Reacquire) {
    unsafe { (*r).reset() }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_reacquire_step(r: *mut Reacquire, tracked: *const HandPose, valid: bool, dt: f32,
    out: *mut HandPose) {
    unsafe { *out = (*r).step(&*tracked, valid, dt) }
}
#[no_mangle]
pub extern "C" fn cvr_exponential_follow(rate: f32, dt: f32) -> f32 {
    tracking::exponential_follow(rate, dt)
}

// ---- AnchorTranslation, HandViewFrame, NativeCameraPair, SceneCameraHeading --------------------------------------------
#[no_mangle]
pub extern "C" fn cvr_anchor_rotate_yaw(v: AnchorVector, yaw: f32) -> AnchorVector {
    anchor::rotate_anchor_yaw(v, yaw)
}
#[no_mangle]
pub extern "C" fn cvr_anchor_compose(head: AnchorVector, tracking: AnchorVector, model: AnchorVector, tracking_yaw: f32,
    model_yaw: f32) -> AnchorVector {
    anchor::compose_anchor_translation(head, tracking, model, tracking_yaw, model_yaw)
}
#[no_mangle]
pub unsafe extern "C" fn cvr_anchor_compose_view(residual: AnchorVector, recipe: *const AnchorRecipe, tracking_yaw: f32,
    body_yaw: f32) -> AnchorVector {
    unsafe { anchor::compose_view_translation(residual, &*recipe, tracking_yaw, body_yaw) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_anchor_orbit_neck_lever(recipe: *const AnchorRecipe, rest_neck: *const AnchorVector) -> AnchorVector {
    unsafe { anchor::body_orbit_neck_lever(&*recipe, rest_neck.as_ref()) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_anchor_orbit_model(recipe: *const AnchorRecipe, body_yaw: f32, tracking_yaw: f32,
    rest_neck: *const AnchorVector) -> AnchorVector {
    unsafe { anchor::body_orbit_model(&*recipe, body_yaw, tracking_yaw, rest_neck.as_ref()) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_anchor_make_recipe(manual: AnchorVector, vehicle: AnchorVector, baked: AnchorVector,
    in_vehicle: bool, scale: f32, tracking_yaw: f32, model_yaw: f32, out: *mut AnchorRecipe) {
    unsafe { *out = anchor::make_anchor_recipe(manual, vehicle, baked, in_vehicle, scale, tracking_yaw, model_yaw) }
}
#[no_mangle]
pub extern "C" fn cvr_anchor_absolute_bake(active: AnchorVector, target: AnchorVector, rendered: AnchorVector) -> AnchorVector {
    anchor::absolute_horizontal_camera_bake(active, target, rendered)
}
#[no_mangle]
pub unsafe extern "C" fn cvr_hand_view_frame(body_base: AnchorVector, recipe: *const AnchorRecipe, residual: AnchorVector,
    head: *const Quat, body_yaw: f32, tracking_yaw: f32, pitch: *const Quat, out: *mut HandViewFrame) {
    unsafe { *out = frames::build_hand_view_frame(body_base, &*recipe, residual, *head, body_yaw, tracking_yaw, *pitch) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_camera_entity_sample(centre: *const [i32; 3], body_base: *const [i32; 3],
    entity: *const [i32; 3], camera_quat: *const [f32; 4], entity_quat: *const [f32; 4], out: *mut CameraEntitySample) {
    unsafe { *out = frames::make_camera_entity_sample(&*centre, &*body_base, &*entity, &*camera_quat, &*entity_quat) }
}
#[no_mangle]
pub extern "C" fn cvr_camera_packet_status(owner: usize, origin: u64, stamp: u64, current_owner: usize,
    current_origin: u64, now: u64, valid: bool, unavailable: bool) -> i32 {
    frames::camera_packet_status(owner, origin, stamp, current_owner, current_origin, now, valid, unavailable) as i32
}
#[no_mangle]
pub extern "C" fn cvr_use_native_main_heading(mounted: bool, tier: i32, device: bool, external_scene: bool) -> bool {
    frames::use_native_main_heading(mounted, tier, device, external_scene)
}
#[no_mangle]
pub unsafe extern "C" fn cvr_composition_base_yaw(composed: *const [f32; 4], head: *const [f32; 4], yaw: *mut f32) -> bool {
    if composed.is_null() || head.is_null() || yaw.is_null() {
        return false;
    }
    match unsafe { frames::composition_base_yaw(&*composed, &*head) } {
        Some(v) => {
            unsafe { *yaw = v };
            true
        }
        None => false,
    }
}

// ---- VehicleStereoHeading, EyeCentreLedger ----------------------------------------------------------------------------
#[no_mangle]
pub unsafe extern "C" fn cvr_vehicle_heading_set_source(h: *mut VehicleStereoHeading, player: usize, camera: usize) {
    unsafe { (*h).set_source(player, camera) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_vehicle_heading_observe(h: *mut VehicleStereoHeading, camera: usize, q: *const [f32; 4],
    entity_q: *const [f32; 4]) -> bool {
    unsafe { (*h).observe(camera, q.as_ref(), entity_q.as_ref()) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_vehicle_heading_for_frame(h: *mut VehicleStereoHeading, epoch: u64, entity_q: *const [f32; 4],
    published_yaw: f32, native_offset: f32, out: *mut Selection) {
    unsafe { *out = (*h).for_frame(epoch, entity_q.as_ref(), published_yaw, native_offset) }
}
#[no_mangle]
pub extern "C" fn cvr_eye_ledger_new() -> *mut EyeCentreLedger {
    Box::into_raw(Box::default())
}
#[no_mangle]
pub unsafe extern "C" fn cvr_eye_ledger_free(l: *mut EyeCentreLedger) {
    if !l.is_null() {
        drop(unsafe { Box::from_raw(l) });
    }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_eye_ledger_publish(l: *const EyeCentreLedger, eye: *const [i32; 3], centre: *const [i32; 3],
    body_base: *const [i32; 3]) {
    unsafe { (*l).publish(*eye, *centre, *body_base) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_eye_ledger_read(l: *const EyeCentreLedger, eye: *const [i32; 3], centre: *mut [i32; 3],
    body_base: *mut [i32; 3]) -> bool {
    match unsafe { (*l).read(*eye) } {
        Some((c, b)) => {
            unsafe {
                *centre = c;
                if let Some(out) = body_base.as_mut() {
                    *out = b;
                }
            }
            true
        }
        None => false,
    }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_eye_ledger_read_world(l: *const EyeCentreLedger, query: *const [f32; 3], centre: *mut [f32; 3],
    body_base: *mut [f32; 3]) -> bool {
    match unsafe { (*l).read_world(*query, !body_base.is_null()) } {
        Some((c, b)) => {
            unsafe {
                *centre = c;
                if let Some(out) = body_base.as_mut() {
                    *out = b;
                }
            }
            true
        }
        None => false,
    }
}

// ---- policies -------------------------------------------------------------------------------------------------------
#[no_mangle]
pub extern "C" fn cvr_should_suspend_vrik(engine_tier: i32, configured_tier: i32) -> bool {
    policy::should_suspend_vrik(engine_tier, configured_tier)
}
#[no_mangle]
pub extern "C" fn cvr_should_suspend_vrik_for_takedown(state: i32, enabled: i32) -> bool {
    policy::should_suspend_vrik_for_takedown(state, enabled)
}
#[no_mangle]
pub extern "C" fn cvr_should_suspend_vrik_for_body_carry(carrying: i32, disposal: i32, locomotion: i32, enabled: i32) -> bool {
    policy::should_suspend_vrik_for_body_carry(carrying, disposal, locomotion, enabled)
}
#[no_mangle]
pub extern "C" fn cvr_should_suspend_vrik_for_finisher(active: i32, age_ms: u64, enabled: i32) -> bool {
    policy::should_suspend_vrik_for_finisher(active, age_ms, enabled)
}
#[no_mangle]
pub extern "C" fn cvr_should_hand_right_arm_to_body_carry(carrying: i32, both_hands: i32, enabled: i32) -> bool {
    policy::should_hand_right_arm_to_body_carry(carrying, both_hands, enabled)
}
#[no_mangle]
pub extern "C" fn cvr_resolve_vehicle_vrik_suspend_tier(on_foot: i32, vehicle: i32, explicit: bool) -> i32 {
    policy::resolve_vehicle_vrik_suspend_tier(on_foot, vehicle, explicit)
}
#[no_mangle]
pub unsafe extern "C" fn cvr_route_turn(axis: f32, snap_enabled: bool, armed: *mut i32, angle: f32, fire: f32, rearm: f32,
    out: *mut policy::TurnInput) {
    unsafe { *out = policy::route_turn(axis, snap_enabled, &mut *armed, angle, fire, rearm) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_vehicle_button_policy(mounted: bool, state: i32, out: *mut policy::VehicleButtons) {
    unsafe { *out = policy::vehicle_button_policy(mounted, state) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_vehicle_exit_hold_update(h: *mut policy::VehicleExitHold, allowed: bool, held: bool, now: u64,
    hold_ms: u64) -> bool {
    unsafe { (*h).update(allowed, held, now, hold_ms) }
}

// ---- PipelinePrediction, RuntimeFovCorrection ----------------------------------------------------------------------
use cvr_camera::projection::{self, Fov, PipelinePrediction, RuntimeFovCorrection};
#[no_mangle]
pub unsafe extern "C" fn cvr_pipeline_prediction_reset(p: *mut PipelinePrediction) {
    unsafe { (*p).reset() }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_pipeline_prediction_target(p: *mut PipelinePrediction, display: i64, period: i64, fitted: i64, valid: bool, strength: f32) -> i64 {
    unsafe { (*p).target(display, period, fitted, valid, strength) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_runtime_fov_correction(left: *const Fov, right: *const Fov, out: *mut RuntimeFovCorrection) {
    unsafe { *out = projection::runtime_fov_correction(&*left, &*right) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_corrected_game_horizontal_fov_deg(corr: *const RuntimeFovCorrection) -> f32 {
    unsafe { projection::corrected_game_horizontal_fov_deg(&*corr) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_panel_half_angles(left: *const Fov, right: *const Fov, half_h: *mut f32, half_v: *mut f32) {
    unsafe {
        let (h, v) = projection::panel_half_angles(&*left, &*right);
        *half_h = h;
        *half_v = v;
    }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_panel_covering_horizontal_fov_deg(left: *const Fov, right: *const Fov, aspect: f32) -> f32 {
    unsafe { projection::panel_covering_horizontal_fov_deg(&*left, &*right, aspect) }
}

// ---- AsymmetricProjection ------------------------------------------------------------------------------------------
use cvr_camera::projection::{EngineProjection, EyeWindow};
#[no_mangle]
pub unsafe extern "C" fn cvr_projection_eye_yaws_against_head(a: *const Quat, b: *const Quat, yaw: *mut f32) {
    unsafe {
        let y = projection::eye_yaws_against_head(*a, *b);
        *yaw = y[0];
        *yaw.add(1) = y[1];
    }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_projection_windows(fov: *const [Fov; 2], eye_orientation: *const [Quat; 2], out: *mut [EyeWindow; 2], guard: f32, target: f32) -> bool {
    unsafe {
        match projection::windows(&*fov, eye_orientation.as_ref(), guard, target) {
            Some(w) => {
                *out = w;
                true
            }
            None => false,
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_projection_to_fov(w: *const EyeWindow, out: *mut Fov) {
    unsafe { *out = projection::to_fov(&*w) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_projection_to_engine(w: *const EyeWindow, out: *mut EngineProjection) {
    unsafe { *out = projection::to_engine(&*w) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_projection_map_ndc_x(n: f32, from: *const EyeWindow, to: *const EyeWindow) -> f32 {
    unsafe { projection::map_ndc_x(n, &*from, &*to) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_projection_map_ndc_y(n: f32, from: *const EyeWindow, to: *const EyeWindow) -> f32 {
    unsafe { projection::map_ndc_y(n, &*from, &*to) }
}
