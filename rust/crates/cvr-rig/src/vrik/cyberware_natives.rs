//! The arm cyberware's and the tracked melee's natives (formerly src/Natives/Cyberware.cpp), registered by
//! src/Natives/Registration.cpp: the frames the CET combat module reads each tick (the implants' chains, the tracked
//! hands for a punch, the throwing hand for a knife), and the switches it asks before it lets the game's own attack run.
use super::camera_link::HeadPose;
use super::cyberware::{
    cvr_cyberware_collision_info, cvr_cyberware_muscle_pulse, cvr_cyberware_read_frame, cvr_cyberware_select, cvr_cyberware_selected_kind,
    cvr_cyberware_set_wire_planes, Frame,
};
use super::cyberware_chord::{cvr_autopilot_chord_take, cvr_cyberware_chord_take};
use super::env::{g_VRBind, g_isInVehicle, GetTickCount64};
use super::gestures::cvr_gestures_publish_equipment;
use super::rig::{VRIK_QuatMul, VRIK_QuatNorm, VRIK_QuatRotateVec};
use super::snapshot::SharedPose;
use super::weapon_arm::{cvr_anim_is_head_aim_weapon_active, cvr_anim_weapon_in_left_hand};
use super::xinput::{cvr_input_weapon_trigger_enabled, VrController};
use crate::wire::{finite, length, v3, Plane, COUNT as WIRE_COUNT};
use core::ffi::c_void;
use cvr_body::cmath::{cos, sin};
use red4ext_abi::dynarray::DynArray;
use red4ext_abi::stack::StackFrame;
use std::sync::OnceLock;

extern "C" {
    static CyberpunkVR_TwoHandActive: i32;
    static CyberpunkVR_CarryLeft: i32;
    static g_menuModeValue: i32;
    fn VRIK_ReadCurrentBodyFrame(worldPosition: *mut f32, worldRotation: *mut f32, trackingYaw: *mut f32) -> bool;
    fn cvr_cpp_pose_scene_tier() -> i32;
    fn cvr_cpp_pose_effective_cutscene_tier() -> i32;
}

/// RED4ext::Vector4.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Vector4 {
    x: f32,
    y: f32,
    z: f32,
    w: f32,
}
const fn v4(x: f32, y: f32, z: f32, w: f32) -> Vector4 {
    Vector4 { x, y, z, w }
}

type Vectors = DynArray<Vector4>;

fn head_aim() -> bool {
    // SAFETY: the Rust rig's own export.
    unsafe { cvr_anim_is_head_aim_weapon_active() }
}
fn weapon_in_left_hand() -> bool {
    // SAFETY: as head_aim.
    unsafe { cvr_anim_weapon_in_left_hand() }
}
fn bind() -> i32 {
    // SAFETY: a plain read of the C++ half's binding mode.
    unsafe { core::ptr::read_volatile(&raw const g_VRBind) }
}
fn menu_mode() -> i32 {
    // SAFETY: as bind.
    unsafe { core::ptr::read_volatile(&raw const g_menuModeValue) }
}
fn two_hand_active() -> bool {
    // SAFETY: as bind.
    unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_TwoHandActive) != 0 }
}
fn mul(a: &[f32; 4], b: &[f32; 4]) -> [f32; 4] {
    let mut o = [0.0f32; 4];
    // SAFETY: four floats in, four out.
    unsafe { VRIK_QuatMul(a.as_ptr(), b.as_ptr(), o.as_mut_ptr()) };
    o
}
fn rotate(q: &[f32; 4], v: &[f32; 3]) -> [f32; 3] {
    let mut o = [0.0f32; 3];
    // SAFETY: as mul.
    unsafe { VRIK_QuatRotateVec(q.as_ptr(), v.as_ptr(), o.as_mut_ptr()) };
    o
}
fn gesture_hand_frame(head: &mut HeadPose, hands: &mut [HeadPose; 2], sequence: &mut u64, stamp: &mut u64, raw: bool) -> bool {
    // SAFETY: OpenXRManager::GetGestureHandFrame through the C++ half (src/Hooks/InputServices.cpp).
    unsafe { super::xr_calls::gesture_hand_frame(head, hands.as_mut_ptr(), sequence, stamp, raw) }
}
/// The script's array, emptied (`DynArray::Clear`: the buffer stays), or None for no output.
unsafe fn cleared<'a>(out: *mut Vectors) -> Option<&'a mut Vectors> {
    let out = out.as_mut()?;
    out.clear();
    Some(out)
}
unsafe fn skip_params(frame: *mut StackFrame) {
    if let Some(frame) = frame.as_mut() {
        frame.finish_params();
    }
}
unsafe fn put<T>(out: *mut T, value: T) {
    if !out.is_null() {
        *out = value;
    }
}

/// Appends the actual tracking-space motion, before any game camera or body pose, wrist offset or IK: a native hit
/// reaction or aim turn must not become another punch merely because the rendered fist moved in the game's world.
/// `weapon_last`: the hands in the melee order -- the other hand, then the weapon's (the left one while the weapon is
/// in the left hand); otherwise left then right, as the implants' frames have them.
fn append_melee_tracking(out: &mut Vectors, header: bool, weapon_last: bool) {
    let mut head = HeadPose::default();
    let mut hands = [HeadPose::default(); 2];
    let (mut sequence, mut stamp) = (0u64, 0u64);
    // The raw (unfiltered) frame in every caller (main 0.2.4), the header or not.
    let valid = gesture_hand_frame(&mut head, &mut hands, &mut sequence, &mut stamp, true);
    // Time belongs to this tracking sample, not the later CET read: adjacent reads can straddle publication only a
    // millisecond apart even though the two poses span a full XR frame, and read time would invent a fast swing.
    if header {
        out.push(v4(
            ((stamp / 1000) & 0xffffff) as f32,
            (sequence & 0xffffff) as f32,
            if two_hand_active() { 1.0 } else { 0.0 },
            0.0,
        ));
    }
    let head_q = head.ori;
    let mut tracking = [Vector4::default(); 2];
    let mut rotations = [Vector4::default(); 2];
    for side in 0..2 {
        if !(valid && hands[side].valid) {
            continue;
        }
        let hand = &hands[side];
        let p = rotate(&head_q, &hand.pos);
        let mut q = mul(&head_q, &hand.ori);
        // SAFETY: four floats.
        unsafe { VRIK_QuatNorm(q.as_mut_ptr()) };
        tracking[side] = v4(
            p[0] + head.pos[0],
            p[1] + head.pos[1],
            p[2] + head.pos[2],
            ((head.origin_serial & 0xffffff) as u32 + 1) as f32,
        );
        rotations[side] = v4(q[0], q[1], q[2], q[3]);
    }
    let first = if weapon_last && weapon_in_left_hand() { 1 } else { 0 };
    out.push(tracking[first]);
    out.push(tracking[1 - first]);
    out.push(rotations[first]);
    out.push(rotations[1 - first]);
}

/// VRMeleeInputFrame() -> array<Vector4>: the tracked hands for a punch (header, two positions, two rotations).
#[no_mangle]
pub unsafe extern "C" fn VRMeleeInputFrame(_this: *mut c_void, frame: *mut StackFrame, out: *mut Vectors, _unused: i64) {
    skip_params(frame);
    let Some(out) = cleared(out) else { return };
    if bind() != 4 || head_aim() {
        return;
    }
    append_melee_tracking(out, true, true);
}
/// VRMeleeThrowEnabled() -> Bool.
#[no_mangle]
pub unsafe extern "C" fn VRMeleeThrowEnabled(_this: *mut c_void, frame: *mut StackFrame, out: *mut bool, _unused: i64) {
    skip_params(frame);
    put(out, bind() == 4 && !head_aim());
}
/// VRWeaponTriggerEnabled() -> Bool.
#[no_mangle]
pub unsafe extern "C" fn VRWeaponTriggerEnabled(_this: *mut c_void, frame: *mut StackFrame, out: *mut bool, _unused: i64) {
    skip_params(frame);
    put(out, cvr_input_weapon_trigger_enabled());
}
/// VRCurrentFingerAction() -> Uint64: the weapon finger's fresh pull for the semi-automatic fire, 0 without one.
#[no_mangle]
pub unsafe extern "C" fn VRCurrentFingerAction(_this: *mut c_void, frame: *mut StackFrame, out: *mut u64, _unused: i64) {
    skip_params(frame);
    let action = super::xinput::cvr_input_current_finger_action();
    if !out.is_null() {
        *out = action;
    }
}
/// VRAutopilotChordTake() -> Bool: once per cyberware chord (the left grip + the left stick's click, held) made in the
/// driver seat.
#[no_mangle]
pub unsafe extern "C" fn VRAutopilotChordTake(_this: *mut c_void, frame: *mut StackFrame, out: *mut bool, _unused: i64) {
    skip_params(frame);
    let taken = cvr_autopilot_chord_take();
    put(out, taken);
}
/// VRWeaponInLeftHand() -> Bool: the weapon in hand is drawn in the LEFT hand; the scripts' support hand is the right.
#[no_mangle]
pub unsafe extern "C" fn VRWeaponInLeftHand(_this: *mut c_void, frame: *mut StackFrame, out: *mut bool, _unused: i64) {
    skip_params(frame);
    put(out, weapon_in_left_hand());
}
/// VRLeftHanded's answer: the left-handed controls are on.
///
/// # Safety
/// The live controls' read.
pub unsafe fn left_handed() -> bool {
    pose_controls()[2] != 0
}
/// The live controls the pose hook reads: [vehicle cutscene suspend tier, ladder grip climb, left-handed].
fn pose_controls() -> [i32; 3] {
    let mut controls = [0i32; 3];
    // SAFETY: the C++ half fills three ints (src/Anim/CharacterRigRust.cpp).
    unsafe { super::live_reads::pose_controls(controls.as_mut_ptr()) };
    controls
}
/// VRLeftHanded() -> Bool: the left-handed controls are on (xr_left_handed), weapon or not.
#[no_mangle]
pub unsafe extern "C" fn VRLeftHanded(_this: *mut c_void, frame: *mut StackFrame, out: *mut bool, _unused: i64) {
    skip_params(frame);
    put(out, pose_controls()[2] != 0);
}
/// VRSceneOwnsBody() -> Bool: the scene tier reaches the threshold that suspends VRIK (on foot or mounted, as the pose
/// hook decides it); CyberpunkVRPort_HandsOnly gives the body back for it.
#[no_mangle]
pub unsafe extern "C" fn VRSceneOwnsBody(_this: *mut c_void, frame: *mut StackFrame, out: *mut bool, _unused: i64) {
    skip_params(frame);
    let tier = cvr_cpp_pose_scene_tier();
    let mounted = SharedPose(31) > 0.5;
    let suspend = cvr_body::policy::should_suspend_vrik_in(tier, mounted, cvr_cpp_pose_effective_cutscene_tier(), pose_controls()[0]);
    put(out, suspend);
}
/// VRPhysicalMelee() -> Bool: the tracked hands own the melee attack -- arm cyberware (1..3) or an ordinary melee
/// weapon (5), in gameplay, without the head-aimed fallback (the pad hook's physicalCyberware condition).
#[no_mangle]
pub unsafe extern "C" fn VRPhysicalMelee(_this: *mut c_void, frame: *mut StackFrame, out: *mut bool, _unused: i64) {
    skip_params(frame);
    let kind = cvr_cyberware_selected_kind();
    put(out, ((1..=3).contains(&kind) || kind == 5) && menu_mode() == 0 && !head_aim());
}
/// VRMeleeThrowFrame() -> array<Vector4>: the throwing hand in tracking space, measured in metres; the velocity maps
/// to game axes only at release, so camera and body turns never create throwing energy.
#[no_mangle]
pub unsafe extern "C" fn VRMeleeThrowFrame(_this: *mut c_void, frame: *mut StackFrame, out: *mut Vectors, _unused: i64) {
    skip_params(frame);
    let Some(out) = cleared(out) else { return };
    if bind() != 4 || menu_mode() != 0 || g_isInVehicle || CyberpunkVR_CarryLeft != 0 || head_aim() {
        return;
    }
    let mut head = HeadPose::default();
    let mut hands = [HeadPose::default(); 2];
    let (mut sequence, mut stamp) = (0u64, 0u64);
    let mut input = VrController::default();
    let (mut p, mut q, mut yaw) = ([0.0f32; 3], [0.0f32; 4], 0.0f32);
    // The hand holding the knife throws it: the left one while the weapon is in the left hand.
    let left = weapon_in_left_hand();
    let hand = if left { 0 } else { 1 };
    if !gesture_hand_frame(&mut head, &mut hands, &mut sequence, &mut stamp, true)
        || !hands[hand].valid
        || !super::xr_calls::controller_state(&mut input)
        || !(if left { input.left_hand_valid } else { input.right_hand_valid })
        || !VRIK_ReadCurrentBodyFrame(p.as_mut_ptr(), q.as_mut_ptr(), &mut yaw)
    {
        return;
    }
    let head_q = head.ori;
    let tracking = rotate(&head_q, &hands[hand].pos);
    out.push(v4(
        ((stamp / 1000) & 0xffffff) as f32,
        (sequence & 0xffffff) as f32,
        (head.origin_serial & 0xffffff) as f32,
        0.0,
    ));
    out.push(v4(tracking[0] + head.pos[0], tracking[1] + head.pos[1], tracking[2] + head.pos[2], 1.0));
    // The other hand's trigger and the throwing hand's grip.
    out.push(v4(
        cos(yaw),
        sin(yaw),
        if left { input.right_trigger } else { input.left_trigger },
        if left { input.left_grip } else { input.right_grip },
    ));
    let ray = [0.0f32, 0.0, -1.0];
    let full_q = mul(&head_q, &hands[hand].ori);
    let forward = rotate(&full_q, &ray);
    out.push(v4(forward[0], forward[1], forward[2], 0.0));
    // Where the head looks and where it looks from, in the same tracking space: the throw aims mostly along it.
    let gaze = rotate(&head_q, &ray);
    out.push(v4(gaze[0], gaze[1], gaze[2], 0.0));
    out.push(v4(head.pos[0], head.pos[1], head.pos[2], 1.0));
}
/// VRMeleeRealTime() -> Float: seconds since the first call (a real clock, not game time).
#[no_mangle]
pub unsafe extern "C" fn VRMeleeRealTime(_this: *mut c_void, frame: *mut StackFrame, out: *mut f32, _unused: i64) {
    skip_params(frame);
    static START: OnceLock<u64> = OnceLock::new();
    let start = *START.get_or_init(|| GetTickCount64());
    put(out, (GetTickCount64() - start) as f32 * 0.001);
}
/// VRMeleeTwoHandedActive() -> Bool.
#[no_mangle]
pub unsafe extern "C" fn VRMeleeTwoHandedActive(_this: *mut c_void, frame: *mut StackFrame, out: *mut bool, _unused: i64) {
    skip_params(frame);
    put(out, bind() == 4 && two_hand_active() && !head_aim());
}

fn read_frame() -> Option<Frame> {
    let mut frame = Frame::ZERO;
    // SAFETY: the Rust rig's own export, into a whole Frame.
    unsafe { cvr_cyberware_read_frame(&mut frame) }.then_some(frame)
}

/// VRCyberwareFrame() -> array<Vector4>: header, body origin, two raw wrists, two raw rotations, both chains, then the
/// tracked hands as VRMeleeInputFrame has them (without its header).
#[no_mangle]
pub unsafe extern "C" fn VRCyberwareFrame(_this: *mut c_void, frame: *mut StackFrame, out: *mut Vectors, _unused: i64) {
    skip_params(frame);
    let Some(out) = cleared(out) else { return };
    let Some(f) = read_frame() else { return };
    out.push(v4(f.family as f32, f.count[0] as f32, f.count[1] as f32, f.epoch as f32));
    // W is a modulo-2^24 millisecond clock (exact in Float), not game time: physical swing speed must not multiply
    // under Sandevistan or time dilation.
    out.push(v4(f.body_origin.x, f.body_origin.y, f.body_origin.z, (f.stamp & 0xffffff) as f32));
    for p in f.wrists {
        out.push(v4(p.x, p.y, p.z, 1.0));
    }
    for q in f.hand_rotation {
        out.push(v4(q[0], q[1], q[2], q[3]));
    }
    for side in 0..2 {
        for i in 0..(f.count[side].max(0) as usize).min(WIRE_COUNT) {
            let p = f.points[side][i];
            out.push(v4(p.x, p.y, p.z, 1.0));
        }
    }
    append_melee_tracking(out, false, false);
}
/// VRCyberwarePoint(side, index: Int32) -> Vector4: one point of a chain (W 1), or zero.
#[no_mangle]
pub unsafe extern "C" fn VRCyberwarePoint(_this: *mut c_void, frame: *mut StackFrame, out: *mut Vector4, _unused: i64) {
    let (mut side, mut index) = (0i32, 0i32);
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut side);
        frame.get_parameter(&mut index);
        frame.finish_params();
    }
    let Some(out) = out.as_mut() else { return };
    *out = Vector4::default();
    if !(0..=1).contains(&side) {
        return;
    }
    let Some(f) = read_frame() else { return };
    if index < 0 || index >= f.count[side as usize] || index as usize >= WIRE_COUNT {
        return;
    }
    let p = f.points[side as usize][index as usize];
    *out = v4(p.x, p.y, p.z, 1.0);
}
/// VRCyberwareWirePlanes(side, epoch: Int32, values: array<Vector4>): the monowire's collision planes, a position and
/// a normal per segment.
#[no_mangle]
pub unsafe extern "C" fn VRCyberwareWirePlanes(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _unused: i64) {
    let (mut side, mut epoch) = (0i32, 0i32);
    let mut values = Vectors::default();
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut side);
        frame.get_parameter(&mut epoch);
        frame.get_parameter(&mut values);
        frame.finish_params();
    }
    let mut planes = [Plane {
        position: v3(0.0, 0.0, 0.0),
        normal: v3(0.0, 0.0, 0.0),
        valid: false,
    }; WIRE_COUNT];
    let given = values.as_slice();
    if given.len() == 2 * WIRE_COUNT {
        for (i, plane) in planes.iter_mut().enumerate() {
            let (p, n) = (given[i * 2], given[i * 2 + 1]);
            plane.position = v3(p.x, p.y, p.z);
            plane.normal = v3(n.x, n.y, n.z);
            let len = length(plane.normal);
            plane.valid = p.w > 0.5 && finite(plane.position) && len.is_finite() && len > 0.5 && len < 1.5;
            if plane.valid {
                plane.normal = plane.normal * (1.0 / len);
            }
        }
        cvr_cyberware_set_wire_planes(side, epoch as u32, planes.as_ptr());
    }
    values.free();
}
/// VRCyberwareMusclePulse(side: Int32, duration: Float).
#[no_mangle]
pub unsafe extern "C" fn VRCyberwareMusclePulse(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _unused: i64) {
    let (mut side, mut duration) = (0i32, 0.0f32);
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut side);
        frame.get_parameter(&mut duration);
        frame.finish_params();
    }
    cvr_cyberware_muscle_pulse(side, duration);
}
/// VRCyberwareSelect(kind: Int32).
#[no_mangle]
pub unsafe extern "C" fn VRCyberwareSelect(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _unused: i64) {
    let mut kind = 0i32;
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut kind);
        frame.finish_params();
    }
    cvr_cyberware_select(kind);
}
/// VRHandEquipment(mask: Int32): which hands hold something.
#[no_mangle]
pub unsafe extern "C" fn VRHandEquipment(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _unused: i64) {
    let mut mask = 0i32;
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut mask);
        frame.finish_params();
    }
    cvr_gestures_publish_equipment(mask);
}
/// VRCyberwareChordTake() -> Bool: once per cyberware chord that fired with the game window behind.
#[no_mangle]
pub unsafe extern "C" fn VRCyberwareChordTake(_this: *mut c_void, frame: *mut StackFrame, out: *mut bool, _unused: i64) {
    skip_params(frame);
    let taken = cvr_cyberware_chord_take();
    put(out, taken);
}
/// VRLauncherControllerAim() -> Bool.
#[no_mangle]
pub unsafe extern "C" fn VRLauncherControllerAim(_this: *mut c_void, frame: *mut StackFrame, out: *mut bool, _unused: i64) {
    skip_params(frame);
    put(out, bind() == 4 && !head_aim());
}
/// VRCyberwareCollisionInfo() -> Vector4.
#[no_mangle]
pub unsafe extern "C" fn VRCyberwareCollisionInfo(_this: *mut c_void, frame: *mut StackFrame, out: *mut Vector4, _unused: i64) {
    skip_params(frame);
    let Some(out) = out.as_mut() else { return };
    let mut values = [0.0f32; 4];
    cvr_cyberware_collision_info(values.as_mut_ptr());
    *out = v4(values[0], values[1], values[2], values[3]);
}
