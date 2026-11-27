//! VRIK: the solve for the player's own bones, formerly src/Anim/CharacterRig.cpp -- forward kinematics, the
//! two-bone arm and leg solves, the hand target and its stop, the torso damping and girdle, the arm scaling that fits
//! the avatar to a measured player, and the body under the HMD.
//!
//! Every function keeps the C++ one's name, signature and C linkage (include/Anim/CharacterRig.hpp declares them
//! `extern "C"`), and its arithmetic operation for operation: tools/vrik_tests runs the same cases against both and
//! compares them. The globals the C++ half shares are defined in shared_globals.rs and declared in env.rs; what the
//! C++ file kept to itself is here (state.rs). Only the player pose path calls in, under g_PlayerPoseStateLock -- hence the plain statics.
#![allow(
    unsafe_op_in_unsafe_fn,
    static_mut_refs,
    non_snake_case,
    non_upper_case_globals,
    clippy::missing_safety_doc,
    clippy::too_many_arguments,
    clippy::needless_range_loop
)]

pub mod action_reads;
pub mod arm;
pub mod background_pad;
pub mod blade_pose;
pub mod body;
pub mod body_heading;
pub mod body_yaw_census;
pub mod body_yaw_follow;
pub mod cache;
pub mod creation_target;
pub mod cyberware;
pub mod cyberware_chord;
pub mod cyberware_natives;
pub mod dlss_res;
pub mod debug_view;
pub mod calibration;
pub mod camera;
pub mod camera_fov;
pub mod camera_link;
pub mod camera_math;
pub mod camera_pitch;
pub mod camera_sites;
pub mod reach_dev;
pub mod entity_attach;
pub mod env;
pub mod final_camera;
pub mod finger_contact;
pub mod fingergrip;
pub mod force_heading_update;
pub mod frame_order;
pub mod free_delta_head;
pub mod gestures;
pub mod grasp;
pub mod hair_dangle;
pub mod hand_body;
pub mod haptics;
pub mod input_prompts;
pub mod nightcoop;
pub mod interaction_action;
pub mod keypad;
pub mod ladder_input;
pub mod left_handed_animation;
pub mod live_reads;
pub mod locate_camera;
pub mod lod_fov;
pub mod menu_mode;
pub mod menu_source;
pub mod menu_surface;
pub mod neck_cap;
pub mod npc_grip;
pub mod vehicle_doors;
pub mod on_foot_delta_head;
pub mod on_foot_move_xy;
pub mod os_input;
pub mod panzer;
pub mod patch_camera;
pub mod performance;
pub mod photo_capture;
pub mod physhands;
pub mod physx_step;
pub mod player_body;
pub mod port_sound;
pub mod pose_hook;
pub mod anim_pose_hook;
pub mod image_pose;
pub mod pose_identity;
pub mod pose_rig;
pub mod optic_hinge;
pub mod pose_transport;
pub mod proj_aspect;
pub mod proj_stage;
pub mod provider;
pub mod projection_commit;
pub mod recoil;
pub mod reload;
pub mod reach;
pub mod rebind_census;
pub mod reflex_hook;
pub mod reload_handoff;
pub mod remote_camera;
pub mod roomscale_move;
pub mod rtti_call;
pub mod save_file;
pub mod save_looks;
pub mod rig;
pub mod seamless_loading;
pub mod settings_res;
pub mod shared_globals;
pub mod shoulder;
pub mod smoking;
pub mod snapshot;
pub mod state;
pub mod state_globals;
pub mod steer_curve;
pub mod story_attention;
pub mod surveillance_body;
pub mod swimming_input;
pub mod two_hand;
pub mod types;
pub mod vehicle_hands_natives;
pub mod vr_menu;
pub mod ui_popup;
pub mod vendor_route;
pub mod view_key;
pub mod vrik_arming;
pub mod weapon_aim;
pub mod weapon_arm;
pub mod world_blend;
pub mod world_touch;
pub mod weapon_rig;
pub mod weapon_wrist;
pub mod wheel_grab;
pub mod wrist_guard;
pub mod xinput;
pub mod xr_calls;

pub const MAX_BONES: usize = 2048;
pub const MAXB: i32 = MAX_BONES as i32;

pub type V3 = [f32; 3];
pub type Q = [f32; 4];
pub const ID: Q = [0.0, 0.0, 0.0, 1.0];
pub const ZERO3: V3 = [0.0; 3];

/// A bone's translation (`boneBuf + i*48`), the QsTransform's Vector4 at +0.
#[inline(always)]
pub unsafe fn bt(buf: *mut u8, i: i32) -> *mut f32 {
    buf.offset(i as isize * 48) as *mut f32
}
/// A bone's rotation (x y z w) at +16.
#[inline(always)]
pub unsafe fn br(buf: *mut u8, i: i32) -> *mut f32 {
    buf.offset(i as isize * 48 + 16) as *mut f32
}
#[inline(always)]
pub unsafe fn a3(p: *const f32) -> V3 {
    [*p, *p.add(1), *p.add(2)]
}
#[inline(always)]
pub unsafe fn a4(p: *const f32) -> Q {
    [*p, *p.add(1), *p.add(2), *p.add(3)]
}
#[inline(always)]
pub unsafe fn put3(p: *mut f32, v: &V3) {
    *p = v[0];
    *p.add(1) = v[1];
    *p.add(2) = v[2];
}
#[inline(always)]
pub unsafe fn put4(p: *mut f32, v: &Q) {
    *p = v[0];
    *p.add(1) = v[1];
    *p.add(2) = v[2];
    *p.add(3) = v[3];
}
/// The 48 bytes of a bone.
#[inline(always)]
pub unsafe fn bone_bytes(buf: *mut u8, i: i32) -> *mut [u8; 48] {
    buf.offset(i as isize * 48) as *mut [u8; 48]
}
/// The first twelve floats of a bone (translation, rotation, scale's xyzw less one).
#[inline(always)]
pub unsafe fn bone12(buf: *mut u8, i: i32) -> *mut [f32; 12] {
    buf.offset(i as isize * 48) as *mut [f32; 12]
}

/// The C++ loops' `for(int k=0;k<3;++k) a[k]-b[k]`.
#[inline(always)]
pub fn sub3(a: &V3, b: &V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
#[inline(always)]
pub fn add3(a: &V3, b: &V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
/// `projectPerp`: v less its component along the unit axis (the C++ lambda; safe for out == v).
#[inline(always)]
pub fn project_perp(v: &V3, axis: &V3) -> V3 {
    let d = v[0] * axis[0] + v[1] * axis[1] + v[2] * axis[2];
    [v[0] - axis[0] * d, v[1] - axis[1] * d, v[2] - axis[2] * d]
}
