//! Climbing by the hands (formerly src/Hooks/LadderInput.cpp): the ladder the game state publishes, both hands sampled
//! into its world and the tracking space, the climber's pull handed to the native ladder input (NativeAction /
//! MixMove), and the gripping hands pinned to their rail or rung for the solve (ConstrainHand). The C++ API
//! (cvr::ladder::*) forwards here from src/Hooks/WaterLadderRust.cpp.
use super::camera_link::{cvr_camera_anchor_recipe_read, HeadPose};
use super::env::{g_headingPitchC, g_headingPitchS, g_isInVehicle, VrikTransformSnapshot};
use super::roomscale_move::{cvr_roomscale_camera_consumed_at, cvr_roomscale_player_identity, now_us};
use crate::ladder::HANDS;
use crate::ladder_climb::{correct_rung_wrist, dot, fresh, length, on_ladder, Climber, Frame, Geometry, Rotation, Vec};
use cvr_body::cmath::{atan2, clamp, cos, sin};
use cvr_body::swim::hand_in_tracking_space;
use cvr_body::xr::{conjugate_quat, multiply_quat, rotate_vector, Quat, Vec3};
use cvr_camera::anchor::{av, AnchorRecipe};
use cvr_camera::frames::build_hand_view_frame;
use std::sync::{LazyLock, Mutex};

extern "C" {
    static g_VRScaleL: f32;
    static g_VRScaleR: f32;
    static g_VRWristL_I: f32;
    static g_VRWristL_J: f32;
    static g_VRWristL_K: f32;
    static g_VRWristL_R: f32;
    static g_VRWristR_I: f32;
    static g_VRWristR_J: f32;
    static g_VRWristR_K: f32;
    static g_VRWristR_R: f32;
    static g_VROffLX: f32;
    static g_VROffLY: f32;
    static g_VROffLZ: f32;
    static g_VROffRX: f32;
    static g_VROffRY: f32;
    static g_VROffRZ: f32;
    fn VRIK_ReadCurrentBodyFrame(p: *mut f32, q: *mut f32, tracking_yaw: *mut f32) -> bool;
    fn VRIK_ReadNativeTransformSnapshot(out: *mut VrikTransformSnapshot) -> bool;
    fn cvr_cpp_pose_scene_tier() -> i32;
    fn cvr_cpp_menu_mode() -> i32;
    fn cvr_cpp_device_cam_active() -> bool;
    fn cvr_cpp_braindance_active() -> bool;
    fn cvr_cpp_ui_popup_open() -> i32;
    fn cvr_cpp_overlay_visible() -> bool;
    fn cvr_cpp_simulator_loaded() -> bool;
    fn cvr_cpp_swimming_script_hooks_active() -> bool;
}

/// The g_liveControls fields the ladder reads.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct LadderControls {
    pub grip_climb: i32,
    pub auto_finish: i32,
    pub finish_distance: f32,
}
fn ladder_controls() -> LadderControls {
    let mut c = LadderControls::default();
    // SAFETY: fills the struct.
    unsafe { super::live_reads::ladder_controls(&mut c) };
    c
}

struct State {
    player: usize,
    stamp: u64,
    input_stamp: u64,
    test_until: u64,
    detailed: i32,
    paused: bool,
    geometry: Geometry,
    climber: Climber,
    axis: f32,
    test_grip: [f32; 2],
    hand_world: [Vec; 2],
}
static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| {
    Mutex::new(State {
        player: 0,
        stamp: 0,
        input_stamp: 0,
        test_until: 0,
        detailed: -1,
        paused: false,
        geometry: Geometry::default(),
        climber: Climber::default(),
        axis: 0.0,
        test_grip: [0.0; 2],
        hand_world: [Vec::default(); 2],
    })
});
fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}
impl State {
    fn is_fresh(&self, now: u64) -> bool {
        fresh(self.player, cvr_roomscale_player_identity(), self.stamp, now, self.detailed)
    }
    fn clear(&mut self) {
        self.climber.reset();
        self.axis = 0.0;
        self.input_stamp = 0;
    }
    fn input_fresh(&self, now: u64) -> bool {
        !self.paused && self.is_fresh(now) && self.input_stamp != 0 && now >= self.input_stamp && now - self.input_stamp <= 100000
    }
}
unsafe fn context_allowed() -> bool {
    let tier = cvr_cpp_pose_scene_tier();
    ladder_controls().grip_climb != 0
        && !g_isInVehicle
        && cvr_cpp_menu_mode() == 0
        && !cvr_cpp_device_cam_active()
        && !cvr_cpp_braindance_active()
        && cvr_cpp_ui_popup_open() == 0
        && !cvr_cpp_overlay_visible()
        && tier > 0
        && tier < 4
}
fn v(v: Vec3) -> Vec {
    Vec { x: v.x, y: v.y, z: v.z }
}

#[no_mangle]
pub extern "C" fn cvr_ladder_active() -> bool {
    state().is_fresh(now_us())
}
#[no_mangle]
pub unsafe extern "C" fn cvr_ladder_read_hand_kinds(kinds: *mut i32) {
    *kinds = 0;
    *kinds.add(1) = 0;
    if !context_allowed() {
        return;
    }
    let s = state();
    let now = now_us();
    if !s.input_fresh(now) {
        return;
    }
    for side in 0..2 {
        if s.climber.held(side) {
            *kinds.add(side as usize) = s.climber.kind(side as usize);
        }
    }
}
/// The ladder the game state publishes (Natives/Ladder.cpp); a new identity keeps nothing, the same one keeps its
/// measured top and finisher rails.
#[no_mangle]
pub unsafe extern "C" fn cvr_ladder_publish(player: usize, detailed: i32, paused: bool, geometry: *const Geometry) {
    let mut s = state();
    if player != s.player || !on_ladder(detailed) || paused {
        s.clear();
    }
    let mut next = if geometry.is_null() { Geometry::default() } else { *geometry };
    if player == s.player && next.valid && s.geometry.identity == next.identity {
        next.measured_top = s.geometry.measured_top;
        next.top_rail_count = s.geometry.top_rail_count;
        let n = next.top_rail_count.clamp(0, 10) as usize;
        next.top_rails[..n].copy_from_slice(&s.geometry.top_rails[..n]);
    }
    s.player = player;
    s.detailed = detailed;
    s.paused = paused;
    s.stamp = now_us();
    s.geometry = next;
}
#[no_mangle]
pub unsafe extern "C" fn cvr_ladder_publish_top(
    player: usize,
    ladder_position: *const Vec,
    origin: *const Vec,
    right: *const Vec,
    normal: *const Vec,
    up: *const Vec,
) -> bool {
    let mut s = state();
    if player != s.player || !s.is_fresh(now_us()) || length(*ladder_position - s.geometry.position) > 0.01 {
        return false;
    }
    s.geometry.set_top_rails(*origin, *right, *normal, *up)
}
#[no_mangle]
pub unsafe extern "C" fn cvr_ladder_publish_measured_top(player: usize, ladder_position: *const Vec, top: f32) -> bool {
    let mut s = state();
    if player != s.player || !s.is_fresh(now_us()) || length(*ladder_position - s.geometry.position) > 0.01 {
        return false;
    }
    s.geometry.set_measured_top(top)
}
#[no_mangle]
pub extern "C" fn cvr_ladder_reset_input() {
    state().clear();
}
/// The simulator's grips for a test, at most 2 s; only with the OpenXR Simulator loaded.
#[no_mangle]
pub unsafe extern "C" fn cvr_ladder_set_simulator_grip(left: f32, right: f32, milliseconds: i32) -> bool {
    if !cvr_cpp_simulator_loaded() {
        return false;
    }
    let mut s = state();
    s.test_grip = [clamp(left, 0.0, 1.0), clamp(right, 0.0, 1.0)];
    s.test_until = if milliseconds > 0 {
        now_us() + milliseconds.min(2000) as u64 * 1000
    } else {
        0
    };
    true
}

/// VRControllerState's grips and hand flags (super::xinput::VrController).
#[no_mangle]
pub unsafe extern "C" fn cvr_ladder_update_input(controllers: *const super::xinput::VrController) {
    let c = *controllers;
    let context = context_allowed();
    let mut grips = [c.left_grip, c.right_grip];
    let geometry;
    {
        let mut s = state();
        let now = now_us();
        if !context || s.paused || !s.is_fresh(now) || !s.geometry.valid {
            s.clear();
            return;
        }
        geometry = s.geometry;
        if now < s.test_until {
            grips = s.test_grip;
        }
    }
    let lc = ladder_controls();
    let mut sample = Frame {
        grip: grips,
        auto_finish: lc.auto_finish != 0,
        finish_distance: lc.finish_distance,
        ..Frame::default()
    };
    let mut head = HeadPose::default();
    let mut hands = [HeadPose::default(); 2];
    let (mut sequence, mut stamp) = (0u64, 0u64);
    let (mut entity_pos, mut entity_rot, mut tracking_yaw) = ([0.0f32; 3], [0.0f32; 4], 0.0f32);
    let mut pair: VrikTransformSnapshot = core::mem::zeroed();
    let mut recipe = AnchorRecipe::default();
    let valid = super::xr_calls::gesture_hand_frame(&mut head, hands.as_mut_ptr(), &mut sequence, &mut stamp, false)
        && VRIK_ReadCurrentBodyFrame(entity_pos.as_mut_ptr(), entity_rot.as_mut_ptr(), &mut tracking_yaw)
        && VRIK_ReadNativeTransformSnapshot(&mut pair)
        && pair.valid != 0
        && pair.unavailable == 0
        && cvr_camera_anchor_recipe_read(&mut recipe);
    if valid {
        let entity_q = Quat {
            x: entity_rot[0],
            y: entity_rot[1],
            z: entity_rot[2],
            w: entity_rot[3],
        };
        let base = rotate_vector(
            conjugate_quat(entity_q),
            Vec3 {
                x: pair.bodyCameraMinusEntity[0],
                y: pair.bodyCameraMinusEntity[1],
                z: pair.bodyCameraMinusEntity[2],
            },
        );
        let consumed = cvr_roomscale_camera_consumed_at(head.origin_serial);
        let body_yaw = 2.0 * atan2(entity_rot[2], entity_rot[3]);
        let head_q = Quat {
            x: head.ori[0],
            y: head.ori[1],
            z: head.ori[2],
            w: head.ori[3],
        };
        let view = build_hand_view_frame(
            av(base.x, base.y, base.z),
            &recipe,
            av(head.pos[0] - consumed.x, head.pos[1], head.pos[2] + consumed.y),
            head_q,
            body_yaw,
            tracking_yaw,
            Quat {
                x: g_headingPitchS,
                y: 0.0,
                z: 0.0,
                w: g_headingPitchC,
            },
        );
        let view_rotation = view.rotation;
        let entity = Vec {
            x: entity_pos[0],
            y: entity_pos[1],
            z: entity_pos[2],
        };
        for side in 0..2 {
            let h = hands[side];
            let scale = if side == 1 { g_VRScaleR } else { g_VRScaleL };
            let local = rotate_vector(
                view_rotation,
                Vec3 {
                    x: h.pos[0] * scale,
                    y: -h.pos[2] * scale,
                    z: h.pos[1] * scale,
                },
            );
            let controller = Quat {
                x: h.ori[0],
                y: -h.ori[2],
                z: h.ori[1],
                w: h.ori[3],
            };
            let correction = if side == 1 {
                Quat {
                    x: g_VRWristR_I,
                    y: g_VRWristR_J,
                    z: g_VRWristR_K,
                    w: g_VRWristR_R,
                }
            } else {
                Quat {
                    x: g_VRWristL_I,
                    y: g_VRWristL_J,
                    z: g_VRWristL_K,
                    w: g_VRWristL_R,
                }
            };
            let wrist = multiply_quat(view_rotation, multiply_quat(controller, correction));
            let palm = HANDS[side].contact;
            let contact = rotate_vector(
                wrist,
                Vec3 {
                    x: palm[0],
                    y: palm[1],
                    z: palm[2],
                },
            );
            let tuning = if side == 1 {
                Vec {
                    x: g_VROffRX,
                    y: g_VROffRY,
                    z: g_VROffRZ,
                }
            } else {
                Vec {
                    x: g_VROffLX,
                    y: g_VROffLY,
                    z: g_VROffLZ,
                }
            };
            sample.world[side] = entity
                + v(rotate_vector(
                    entity_q,
                    Vec3 {
                        x: view.position.x + local.x + tuning.x + contact.x,
                        y: view.position.y + local.y + tuning.y + contact.y,
                        z: view.position.z + local.z + tuning.z + contact.z,
                    },
                ));
            let tracking = hand_in_tracking_space(
                Vec3 {
                    x: head.pos[0],
                    y: head.pos[1],
                    z: head.pos[2],
                },
                head_q,
                Vec3 {
                    x: h.pos[0],
                    y: h.pos[1],
                    z: h.pos[2],
                },
            );
            sample.tracking[side] = Vec {
                x: tracking.x * recipe.scale,
                y: -tracking.z * recipe.scale,
                z: tracking.y * recipe.scale,
            };
            sample.hand_valid[side] = h.valid && if side == 1 { c.right_hand_valid } else { c.left_hand_valid };
        }
        let (cy, sy) = (cos(tracking_yaw), sin(tracking_yaw));
        sample.tracking_up = Vec {
            x: cy * geometry.up.x + sy * geometry.up.y,
            y: -sy * geometry.up.x + cy * geometry.up.y,
            z: geometry.up.z,
        };
        sample.height = dot(entity - geometry.position, geometry.up);
        sample.sequence = sequence;
        sample.origin = head.origin_serial;
        sample.stamp = stamp;
        sample.valid = head.valid;
    }
    let mut s = state();
    let now = now_us();
    if !s.is_fresh(now) || s.paused || geometry.identity != s.geometry.identity {
        s.clear();
        return;
    }
    s.hand_world = sample.world;
    let g = s.geometry;
    s.axis = s.climber.update(&g, &sample, now, valid);
    s.input_stamp = now;
}
#[no_mangle]
pub unsafe extern "C" fn cvr_ladder_native_action() -> f32 {
    if !context_allowed() {
        return 0.0;
    }
    let s = state();
    if !s.input_fresh(now_us()) {
        return 0.0;
    }
    s.axis
}
#[no_mangle]
pub unsafe extern "C" fn cvr_ladder_mix_move(original: f32, raw_x: f32, raw_y: f32) -> f32 {
    if raw_x.abs() > 0.15 || raw_y.abs() > 0.15 {
        cvr_ladder_reset_input();
        return original;
    }
    let axis = cvr_ladder_native_action();
    if axis != 0.0 {
        axis
    } else {
        original
    }
}
/// The gripping hand pinned to its rail or rung, in the one body frame the caller latched for both hands and the view.
#[no_mangle]
pub unsafe extern "C" fn cvr_ladder_constrain_hand(side: i32, target_model: *mut f32, hand_rotation: *mut f32, position: *const f32, rotation: *const f32) {
    if target_model.is_null() || hand_rotation.is_null() || position.is_null() || rotation.is_null() || !(0..=1).contains(&side) || !context_allowed() {
        return;
    }
    let s = state();
    if !s.input_fresh(now_us()) {
        return;
    }
    let mut point = Vec {
        x: *target_model,
        y: *target_model.add(1),
        z: *target_model.add(2),
    };
    let mut wrist = Rotation {
        x: *hand_rotation,
        y: *hand_rotation.add(1),
        z: *hand_rotation.add(2),
        w: *hand_rotation.add(3),
    };
    let body_rotation = Rotation {
        x: *rotation,
        y: *rotation.add(1),
        z: *rotation.add(2),
        w: *rotation.add(3),
    };
    if s.climber.constrain(
        side,
        Vec {
            x: *position,
            y: *position.add(1),
            z: *position.add(2),
        },
        body_rotation,
        &mut point,
        &mut wrist,
    ) {
        if s.climber.kind(side as usize) == 2 {
            let mut body = body_rotation;
            body.normalize();
            correct_rung_wrist(
                side,
                body.inverse().rotate(s.geometry.right),
                body.inverse().rotate(s.geometry.up),
                &mut point,
                &mut wrist,
            );
        }
        *target_model = point.x;
        *target_model.add(1) = point.y;
        *target_model.add(2) = point.z;
        *hand_rotation = wrist.x;
        *hand_rotation.add(1) = wrist.y;
        *hand_rotation.add(2) = wrist.z;
        *hand_rotation.add(3) = wrist.w;
    }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_ladder_debug_value(index: i32) -> f32 {
    if index == 20 {
        return cvr_ladder_native_action();
    }
    let s = state();
    if (10..16).contains(&index) {
        let p = s.hand_world[((index - 10) / 3) as usize];
        return match (index - 10) % 3 {
            0 => p.x,
            1 => p.y,
            _ => p.z,
        };
    }
    match index {
        0 => s.detailed as f32,
        1 => s.is_fresh(now_us()) as i32 as f32,
        2 => s.axis,
        3 => s.climber.held(0) as i32 as f32,
        4 => s.climber.held(1) as i32 as f32,
        5 => s.climber.debt(),
        6 => s.geometry.valid as i32 as f32,
        7 => s.climber.kind(0) as f32,
        8 => s.climber.kind(1) as f32,
        9 => {
            if s.stamp != 0 {
                now_us().wrapping_sub(s.stamp) as f32 * 0.001
            } else {
                -1.0
            }
        }
        16 => s.geometry.top_rail_count as f32,
        17 => s.geometry.physical_top(),
        18 => s.climber.finishing() as i32 as f32,
        21 => cvr_cpp_swimming_script_hooks_active() as i32 as f32,
        _ => -1.0,
    }
}

// The ladder module's natives (formerly src/Natives/Ladder.cpp), registered by src/Natives/Registration.cpp.

/// RED4ext::Vector4.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Vector4 {
    x: f32,
    y: f32,
    z: f32,
    w: f32,
}
fn vec3(v: Vector4) -> Vec {
    Vec { x: v.x, y: v.y, z: v.z }
}
type VmFrame = red4ext_abi::stack::StackFrame;
type ScriptHandle = red4ext_abi::handle::Handle;

/// gamestateMachineparameterTypeLadderDescription: the ladder's frame and steps (IScriptable, 0x90 bytes).
const LADDER_DESCRIPTION: red4ext_abi::cname::CName = red4ext_abi::cname::CName::hashed("gamestateMachineparameterTypeLadderDescription");

/// The geometry a LadderDescription gives, prepared (valid or not), or the default when the object is another type.
unsafe fn description_geometry(description: &ScriptHandle) -> Geometry {
    let mut geometry = Geometry::default();
    let instance = description.instance;
    if instance.is_null() {
        return geometry;
    }
    type GetType = unsafe extern "C" fn(*mut core::ffi::c_void) -> *const red4ext_abi::rtti::IType;
    // ISerializable::GetType, vtable +0x08.
    let ty = red4ext_abi::rtti::vfunc::<GetType>(instance, 0x08)(instance);
    if ty.is_null() || (*ty).name() != LADDER_DESCRIPTION {
        return geometry;
    }
    let base = instance as *const u8;
    let vector = |at: usize| vec3(core::ptr::read_unaligned(base.add(at) as *const Vector4));
    let float = |at: usize| core::ptr::read_unaligned(base.add(at) as *const f32);
    geometry.position = vector(0x40);
    geometry.normal = vector(0x50);
    geometry.up = vector(0x60);
    geometry.height = float(0x70); // topHeightFromPosition
    geometry.top_step = float(0x78); // verticalStepTop
    geometry.first_rung = float(0x80); // verticalStepBottom
    let mut hash: u64 = 14695981039346656037;
    for value in [
        geometry.position.x,
        geometry.position.y,
        geometry.position.z,
        geometry.normal.x,
        geometry.normal.y,
        geometry.normal.z,
        geometry.height,
        geometry.top_step,
    ] {
        hash ^= (value * 1000.0).round() as i64 as u64; // std::llround
        hash = hash.wrapping_mul(1099511628211);
    }
    geometry.identity = hash;
    geometry.prepare();
    geometry
}

/// SetVRLadderState(player, detailed: Int32, paused: Int32, description: handle:LadderDescription) -> Int32 (1).
#[no_mangle]
pub unsafe extern "C" fn SetVRLadderState(_this: *mut core::ffi::c_void, frame: *mut VmFrame, out: *mut i32, _unused: i64) {
    let (mut player, mut description) = (ScriptHandle::null(), ScriptHandle::null());
    let (mut detailed, mut paused) = (-1i32, 0i32);
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut player);
        frame.get_parameter(&mut detailed);
        frame.get_parameter(&mut paused);
        frame.get_parameter(&mut description);
        frame.finish_params();
    }
    let geometry = description_geometry(&description);
    cvr_ladder_publish(
        player.instance as usize,
        detailed,
        paused != 0,
        if geometry.valid { &geometry } else { core::ptr::null() },
    );
    if !out.is_null() {
        *out = 1;
    }
}
/// SetVRLadderState's publication from Rust (rust/crates/cvr-ffi/src/mechanics.rs ladder).
///
/// # Safety
/// `description` is null or a live scriptable object, read on the main thread.
pub unsafe fn publish_state(player: usize, detailed: i32, paused: bool, description: &ScriptHandle) {
    // SAFETY: as SetVRLadderState reads it.
    let geometry = unsafe { description_geometry(description) };
    // SAFETY: the ladder state's publication.
    unsafe { cvr_ladder_publish(player, detailed, paused, if geometry.valid { &geometry } else { core::ptr::null() }) };
}
/// The description's position, up and height above the position (LadderDescription's fields), None for another type.
///
/// # Safety
/// As `publish_state`.
pub unsafe fn description_frame(description: &ScriptHandle) -> Option<([f32; 4], [f32; 4], f32)> {
    let instance = description.instance;
    if instance.is_null() {
        return None;
    }
    type GetType = unsafe extern "C" fn(*mut core::ffi::c_void) -> *const red4ext_abi::rtti::IType;
    // SAFETY: ISerializable::GetType, vtable +0x08, then the type's fields.
    unsafe {
        let ty = red4ext_abi::rtti::vfunc::<GetType>(instance, 0x08)(instance);
        if ty.is_null() || (*ty).name() != LADDER_DESCRIPTION {
            return None;
        }
        let base = instance as *const u8;
        let v = |at: usize| {
            let v = core::ptr::read_unaligned(base.add(at) as *const Vector4);
            [v.x, v.y, v.z, v.w]
        };
        Some((v(0x40), v(0x60), core::ptr::read_unaligned(base.add(0x70) as *const f32)))
    }
}
/// SetVRLadderTopRails(player, ladder, origin, right, normal, up: Vector4) -> Int32.
#[no_mangle]
pub unsafe extern "C" fn SetVRLadderTopRails(_this: *mut core::ffi::c_void, frame: *mut VmFrame, out: *mut i32, _unused: i64) {
    let mut player = ScriptHandle::null();
    let mut v = [Vector4::default(); 5];
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut player);
        for value in &mut v {
            frame.get_parameter(value);
        }
        frame.finish_params();
    }
    let [ladder, origin, right, normal, up] = v.map(vec3);
    let ok = cvr_ladder_publish_top(player.instance as usize, &ladder, &origin, &right, &normal, &up);
    if !out.is_null() {
        *out = ok as i32;
    }
}
/// SetVRLadderMeasuredTop(player, ladderPosition: Vector4, top: Float) -> Int32: where the ladder's highest module
/// ends above its foot.
#[no_mangle]
pub unsafe extern "C" fn SetVRLadderMeasuredTop(_this: *mut core::ffi::c_void, frame: *mut VmFrame, out: *mut i32, _unused: i64) {
    let mut player = ScriptHandle::null();
    let mut ladder = Vector4::default();
    let mut top = 0.0f32;
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut player);
        frame.get_parameter(&mut ladder);
        frame.get_parameter(&mut top);
        frame.finish_params();
    }
    let ok = cvr_ladder_publish_measured_top(player.instance as usize, &vec3(ladder), top);
    if !out.is_null() {
        *out = ok as i32;
    }
}
/// GetVRLadderDebug(index: Int32) -> Float.
#[no_mangle]
pub unsafe extern "C" fn GetVRLadderDebug(_this: *mut core::ffi::c_void, frame: *mut VmFrame, out: *mut f32, _unused: i64) {
    let mut index = 0i32;
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut index);
        frame.finish_params();
    }
    if !out.is_null() {
        *out = cvr_ladder_debug_value(index);
    }
}
/// SetVRLadderTestGrip(left: Float, right: Float, milliseconds: Int32) -> Int32 (the simulator's grips).
#[no_mangle]
pub unsafe extern "C" fn SetVRLadderTestGrip(_this: *mut core::ffi::c_void, frame: *mut VmFrame, out: *mut i32, _unused: i64) {
    let (mut left, mut right, mut milliseconds) = (0.0f32, 0.0f32, 0i32);
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut left);
        frame.get_parameter(&mut right);
        frame.get_parameter(&mut milliseconds);
        frame.finish_params();
    }
    let ok = cvr_ladder_set_simulator_grip(left, right, milliseconds);
    if !out.is_null() {
        *out = ok as i32;
    }
}
