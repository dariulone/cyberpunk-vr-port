//! The body's turn and bend (stage 3, rust/crates/cvr-body), for the C++ half: the classes of
//! include/Runtimes/{BodyFollowZone,BodyYawFollower,BodyYawPrediction,TrackedBodyYaw,HybridBodyYaw,LookDownCone}.hpp
//! and include/Anim/BodyBend.hpp keep their fields, and with CVR_RUST_BODY their methods call these with `this`.
//! The Rust types have the C++ classes' layouts (sizes asserted on both sides).
//!
//! # Safety
//! Every pointer is a live, properly aligned object of the named C++ class, used by one thread at a time, as the C++
//! methods were.
#![allow(clippy::missing_safety_doc)]
use cvr_body::bend::{self, BendSample, BendTracker};
use cvr_body::follow_zone::BodyFollowZone;
use cvr_body::hybrid_yaw::{self, HybridBodyYaw, HybridCommand, HybridInput, RotationMode};
use cvr_body::look_down;
use cvr_body::prediction::BodyYawPrediction;
use cvr_body::tracked_yaw::{self, TrackedBodyYaw, TrackingFrame, YawEstimate};
use cvr_body::yaw_follower::BodyYawFollower;

// ---- BodyFollowZone ------------------------------------------------------------------------------------------------
#[no_mangle]
pub unsafe extern "C" fn cvr_body_follow_zone_reset(zone: *mut BodyFollowZone) {
    unsafe { (*zone).reset() }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_body_follow_zone_following(zone: *const BodyFollowZone) -> bool {
    unsafe { (*zone).following() }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_body_follow_zone_step(zone: *mut BodyFollowZone, residual: f32, radius: f32, dt: f32) -> f32 {
    unsafe { (*zone).step(residual, radius, dt) }
}

// ---- BodyYawFollower -----------------------------------------------------------------------------------------------
#[no_mangle]
pub unsafe extern "C" fn cvr_body_yaw_follower_set_enabled(f: *mut BodyYawFollower, enabled: bool) {
    unsafe { (*f).set_enabled(enabled) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_body_yaw_follower_reset(f: *mut BodyYawFollower) {
    unsafe { (*f).reset() }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_body_yaw_follower_observe(f: *mut BodyYawFollower, offset: f32, origin: u64) {
    unsafe { (*f).observe(offset, origin) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_body_yaw_follower_step(f: *mut BodyYawFollower, head_yaw: f32, cone: f32, stamp: u64,
    origin: u64) -> f32 {
    unsafe { (*f).step(head_yaw, cone, stamp, origin) }
}

// ---- BodyYawPrediction ---------------------------------------------------------------------------------------------
#[no_mangle]
pub unsafe extern "C" fn cvr_body_yaw_prediction_reset(p: *mut BodyYawPrediction, seed: f32) {
    unsafe { (*p).reset(seed) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_body_yaw_prediction_suspend(p: *mut BodyYawPrediction) {
    unsafe { (*p).suspend() }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_body_yaw_prediction_update(p: *mut BodyYawPrediction, measured: f32, dt: f32,
    accepted: bool) -> f32 {
    unsafe { (*p).update(measured, dt, accepted) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_body_yaw_prediction_sample(p: *const BodyYawPrediction, age_seconds: f32) -> f32 {
    unsafe { (*p).sample(age_seconds) }
}

// ---- TrackedBodyYaw ------------------------------------------------------------------------------------------------
#[no_mangle]
pub unsafe extern "C" fn cvr_tracking_frame_fresh(frame: *const TrackingFrame, current_origin: u64, now_us: u64) -> bool {
    unsafe { tracked_yaw::tracking_frame_fresh(&*frame, current_origin, now_us) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_tracked_body_yaw_reset(t: *mut TrackedBodyYaw, seed: f32, use_absolute_initial: bool) {
    unsafe { (*t).reset(seed, use_absolute_initial) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_tracked_body_yaw_suspend(t: *mut TrackedBodyYaw) {
    unsafe { (*t).suspend() }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_tracked_body_yaw_predict(t: *const TrackedBodyYaw, now_us: u64, out: *mut YawEstimate) {
    unsafe { *out = (*t).predict(now_us) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_tracked_body_yaw_update(t: *mut TrackedBodyYaw, frame: *const TrackingFrame,
    out: *mut YawEstimate) {
    unsafe { *out = (*t).update(&*frame) }
}

// ---- HybridBodyYaw -------------------------------------------------------------------------------------------------
#[no_mangle]
pub extern "C" fn cvr_rotation_mode_from_flags(physical: i32, tracked: i32, hybrid: i32, head_led: i32) -> i32 {
    hybrid_yaw::rotation_mode_from_flags(physical, tracked, hybrid, head_led) as i32
}
#[no_mangle]
pub extern "C" fn cvr_needs_body_tracking(mode: i32) -> bool {
    mode == RotationMode::Tracked as i32 || mode == RotationMode::Hybrid as i32
}
#[no_mangle]
pub unsafe extern "C" fn cvr_hybrid_body_yaw_reset(h: *mut HybridBodyYaw) {
    unsafe { (*h).reset() }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_hybrid_body_yaw_suspend(h: *mut HybridBodyYaw, keep_alignment: bool) {
    unsafe { (*h).suspend(keep_alignment) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_hybrid_body_yaw_step(h: *mut HybridBodyYaw, input: *const HybridInput,
    out: *mut HybridCommand) {
    unsafe { *out = (*h).step(&*input) }
}

// ---- LookDownCone --------------------------------------------------------------------------------------------------
#[no_mangle]
pub extern "C" fn cvr_head_down_degrees(x: f32, y: f32, z: f32, w: f32) -> f32 {
    look_down::head_down_degrees(x, y, z, w)
}
#[no_mangle]
pub extern "C" fn cvr_look_down_free_look_cone(down: f32, normal: f32, lowered: f32, bend: f32) -> f32 {
    look_down::look_down_free_look_cone(down, normal, lowered, bend)
}
#[no_mangle]
pub extern "C" fn cvr_body_free_look_cone(swimming: bool, swimming_degrees: f32, down: f32, normal: f32, lowered: f32,
    bend: f32) -> f32 {
    look_down::body_free_look_cone(swimming, swimming_degrees, down, normal, lowered, bend)
}

// ---- BodyBend ------------------------------------------------------------------------------------------------------
#[no_mangle]
pub extern "C" fn cvr_body_smooth_range(value: f32, from: f32, to: f32) -> f32 {
    bend::smooth_range(value, from, to)
}
#[no_mangle]
pub extern "C" fn cvr_body_bend_weight(angle: f32) -> f32 {
    bend::bend_weight(angle)
}
#[no_mangle]
pub extern "C" fn cvr_body_hinge_drop(angle: f32) -> f32 {
    bend::hinge_drop(angle)
}
#[no_mangle]
pub unsafe extern "C" fn cvr_bend_tracker_set_squat_depth(t: *mut BendTracker, depth: f32) {
    unsafe { (*t).set_squat_depth(depth) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_bend_tracker_neck_drop(t: *const BendTracker) -> f32 {
    unsafe { (*t).neck_drop() }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_bend_tracker_update(t: *mut BendTracker, head: *const [f32; 3], q: *const [f32; 4],
    origin: u64, swimming: bool, walking: bool, out: *mut BendSample) {
    unsafe { *out = (*t).update(*head, *q, origin, swimming, walking) }
}
