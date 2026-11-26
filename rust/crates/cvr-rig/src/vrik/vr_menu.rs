//! The VR menu's natives: the summon gesture, the two pointers, and the input the menu holds back (formerly
//! src/Natives/VrMenu.cpp; the file keeps the services this asks of the C++ half -- the live controls, the native
//! overlay, the settings pages' recorder -- and its `cvr::vrmenu` functions forward here).
//!
//! The menu itself is the game's own UI on a plane in the world (vrport\ui\vr_menu.ent, built and run by
//! mods/cet/CyberpunkVRPort_Menu). What script cannot see is the hands: their tracking history for the gesture, and
//! where the rendered index fingertip and the hand's pointing ray are this frame. These hand that over, and the Lua
//! decides what a touch or a click means.
//!
//! THE GESTURE is the right hand, GRIP HELD, swept straight down (crate::menu_swipe), read in TRACKING space from the
//! published gesture frame, so a turn of the body or the camera never makes a swipe; a grip on the steering wheel, a
//! ladder or a thing in the fingers is not held for it. xr_menu_gesture turns it off (L3 + R3 still opens the menu),
//! and so does reading the grip from the fingers (a hand closed on the controller grips all the time).
//!
//! THE POINTERS are measured on the hand the player SEES: the solved rig's snapshot put in the world by the same
//! model-to-world pair VRPalmWorldPos uses. The fingertip is the end of the index's last phalanx; the ray starts at the
//! index knuckle and runs along the HAND bone (a trigger press does not bend it), placed on the controller's own wrist
//! (g_VRIKHandAim, before the reach and the blends): with the controller beyond the arm's reach the drawn hand stops
//! short and moves with the shoulders, the controller does not.
#![allow(unsafe_op_in_unsafe_fn)]
use super::camera_link::HeadPose;
use super::env::{g_VRFKSnapCount, g_VRFKSnapPos, g_VRFKSnapRot, g_VRIKHandAim, GetTickCount64};
use super::fingergrip::cvr_fingergrip_grip_from_fingers;
use super::grasp::cvr_anim_grasp_holding;
use super::ladder_input::cvr_ladder_active;
use super::port_sound::{cvr_sound_channel, cvr_sound_play_once, Sound};
use super::remote_camera::cvr_in_braindance;
use super::rig::{VRIK_QuatConj, VRIK_QuatMul, VRIK_QuatNorm, VRIK_QuatRotateVec};
use super::wheel_grab::cvr_anim_wheel_control_state;
use super::xinput::VrController;
use crate::menu_swipe::{SwipeDetector, SwipeSample};
use core::ffi::{c_char, c_void};
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering::*};
use red4ext_abi::cstring::CString;
use red4ext_abi::dynarray::DynArray;
use red4ext_abi::stack::StackFrame;
use std::sync::Mutex;

#[allow(non_upper_case_globals)]
extern "C" {
    static g_VRViewWorldPos: [f32; 3];
    static g_VRViewWorldRot: [f32; 4];
    static g_VRViewWorldValid: i32;
    static g_VRCamModelPos: [f32; 3];
    static g_VRCamModelRot: [f32; 4];
    static g_VRPalmModelValid: i32;
    /// The right hand's blend onto the steering wheel (a float's bits).
    static g_wheelBlendRight: AtomicU32;
    fn cvr_cpp_xr_raw_controller_state(out: *mut VrController) -> bool;
    fn cvr_cpp_fbt_calibrating() -> bool;
    /// xr_menu_gesture, xr_vr_weapon_wheel.
    fn cvr_cpp_menu_controls(gesture: *mut i32, weapon_wheel: *mut i32);
    fn cvr_cpp_vrui_consume_menu_summon() -> bool;
    fn cvr_cpp_vrui_visible() -> bool;
    fn cvr_cpp_vrui_toggle();
    fn cvr_cpp_uirec_request(page: i32);
    /// The latest recorded page into `out` (at most `capacity` bytes); its full length.
    fn cvr_cpp_uirec_latest(page: i32, out: *mut u8, capacity: usize) -> usize;
    fn cvr_cpp_uirec_queue_edit(page: i32, id: *const c_char, value: f64);
    fn cvr_cpp_lobby_world_shown() -> bool;
    fn cvr_cpp_swimming_active() -> bool;
    static g_menuModeValue: i32;
    static g_isInVehicle: bool;
    static CyberpunkVR_TwoHandActive: i32;
}

/// VRHandCollision() -> Int32 (main's VrMenu.cpp): script services for live PhysX hands. Bits 0/1 allow that hand's
/// grips, bit 2 publishes nearby people, bit 3 allows object interaction, bit 4 is retired, bit 5 allows prop grabs, bit
/// 6 grasping (a live PhysX hand). No hand in a menu, the lobby, on a ladder or in water, where hands are placed for other
/// reasons; none that holds what the port put in it (a cigarette has to reach the mouth), a steering wheel, or the gun
/// as its support hand: the hold puts that hand on the weapon, and anything pushing it would take it off the grip.
pub unsafe fn hand_collision_mask() -> i32 {
    super::frame_order::note(b'L');
    let mut mask = 0;
    // The scanner slows world physics, not controller tracking. Release script contacts/grabs too; otherwise turning
    // off the native bodies would enable the script's slow-motion push capsules.
    if !cvr_input::scanner::SCANNER_HAND_PHYSICS.active()
        && cvr_settings::lc!(xrHandPhysics) != 0
        && cvr_settings::lc!(xrHandCollision) != 0
        && g_menuModeValue == 0
        && !cvr_cpp_lobby_world_shown()
        && !cvr_ladder_active()
        && !cvr_cpp_swimming_active()
        && (!g_isInVehicle || cvr_settings::lc!(xrHandCollisionVehicle) != 0)
    {
        let wheel = cvr_anim_wheel_control_state();
        let support = if CyberpunkVR_TwoHandActive != 0 { i32::from(super::weapon_arm::cvr_anim_weapon_in_left_hand()) } else { -1 };
        for side in 0..2i32 {
            if super::physhands::on(side as usize)
                && side != support
                && !super::gestures::cvr_gestures_port_holds(side)
                && (wheel & (if side == 1 { 1 } else { 2 })) == 0
            {
                mask |= 1 << side;
            }
        }
        // the people near (NPC capsules) and V's own body: on foot, and seated with the hands' collision there on (the
        // passengers, someone at the window)
        if !g_isInVehicle || cvr_settings::lc!(xrHandCollisionVehicle) != 0 {
            mask |= 4;
        }
        if !g_isInVehicle && cvr_settings::lc!(xrHandPush) != 0 {
            mask |= 8;
        }
        // Grasping is available only for a live PhysX hand. The old script collision solver and template push capsules
        // are no longer a fallback.
        if super::physhands::on(0) || super::physhands::on(1) {
            mask |= 64;
        }
        // 16 was the fingers stopping at surfaces, rays along each finger from the script; removed 2026-11-08.
        if !g_isInVehicle && cvr_settings::lc!(xrPropGrab) != 0 {
            mask |= 32;
        }
    }
    mask
}
#[no_mangle]
pub unsafe extern "C" fn VRHandCollision(_this: *mut c_void, frame: *mut StackFrame, out: *mut i32, _: i64) {
    if let Some(frame) = frame.as_mut() {
        frame.finish_params();
    }
    let mask = hand_collision_mask();
    if !out.is_null() {
        *out = mask;
    }
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

/// Bone indices of the player rig (the snapshot is indexed like it; see the hand collision's capsules.lua).
const HAND: [i32; 2] = [23, 24]; // LeftHand, RightHand
const INDEX1: [i32; 2] = [46, 51]; // LeftHandIndex1, RightHandIndex1
const INDEX3: [i32; 2] = [65, 69]; // LeftHandIndex3, RightHandIndex3
/// From the bone to the end of the finger, along the bone's X: the last capsule's length.
const TIP: f32 = 0.024;

static BLINK: AtomicU32 = AtomicU32::new(0);
static BLINK_UPDATED: AtomicU64 = AtomicU64::new(0);
static CAPTURE: AtomicBool = AtomicBool::new(false);
/// The wheel script's last call (it calls every frame it can open the wheel).
static WHEEL_BEAT: AtomicU64 = AtomicU64::new(0);
/// The hold as the input merge last decided it: 0 none, 1 pressed and undecided, 2 the wheel's hold.
static WHEEL_HOLD: AtomicI32 = AtomicI32::new(0);
struct Gesture {
    swipe: SwipeDetector,
    last_sequence: u64,
}
static GESTURE: Mutex<Option<Gesture>> = Mutex::new(None);

fn rotate(q: &[f32; 4], v: &[f32; 3]) -> [f32; 3] {
    let mut o = [0.0f32; 3];
    // SAFETY: four floats and three in, three out.
    unsafe { VRIK_QuatRotateVec(q.as_ptr(), v.as_ptr(), o.as_mut_ptr()) };
    o
}
fn dot(a: &[f32; 3], b: &[f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn controls() -> (i32, i32) {
    let (mut gesture, mut wheel) = (0, 0);
    // SAFETY: two out-parameters.
    unsafe { cvr_cpp_menu_controls(&mut gesture, &mut wheel) };
    (gesture, wheel)
}

/// The view's model-to-world rotation, when the palm frame and the view are known.
unsafe fn model_to_world_frame() -> Option<[f32; 4]> {
    if g_VRPalmModelValid == 0 || g_VRViewWorldValid == 0 {
        return None;
    }
    let (cam, view) = (g_VRCamModelRot, g_VRViewWorldRot);
    let mut cam_inv = [0.0f32; 4];
    VRIK_QuatConj(cam.as_ptr(), cam_inv.as_mut_ptr());
    let mut m2w = [0.0f32; 4];
    VRIK_QuatMul(view.as_ptr(), cam_inv.as_ptr(), m2w.as_mut_ptr());
    VRIK_QuatNorm(m2w.as_mut_ptr());
    Some(m2w)
}
unsafe fn model_to_world(m2w: &[f32; 4], model: &[f32; 3]) -> [f32; 3] {
    let cam = g_VRCamModelPos;
    let rel = [model[0] - cam[0], model[1] - cam[1], model[2] - cam[2]];
    let w = rotate(m2w, &rel);
    let view = g_VRViewWorldPos;
    [view[0] + w[0], view[1] + w[1], view[2] + w[2]]
}
/// A bone of the snapshot in the world, at `offset` (bone-local).
unsafe fn bone_world(m2w: &[f32; 4], bone: i32, offset: &[f32; 3]) -> Option<[f32; 3]> {
    if bone < 0 || bone >= g_VRFKSnapCount {
        return None;
    }
    let (p, q) = (
        (*(&raw const g_VRFKSnapPos))[bone as usize],
        (*(&raw const g_VRFKSnapRot))[bone as usize],
    );
    let o = rotate(&q, offset);
    let model = [p[0] + o[0], p[1] + o[1], p[2] + o[2]];
    Some(model_to_world(m2w, &model))
}
/// The pointing ray from the controller's own wrist: the index knuckle and the hand's axis put on it the way the
/// snapshot has them on the drawn hand.
unsafe fn aim_world(m2w: &[f32; 4], side: usize, along: &[f32; 3]) -> Option<([f32; 3], [f32; 3])> {
    let aim = (*(&raw const g_VRIKHandAim))[if side != 0 { 0 } else { 1 }];
    let (hand, index) = (HAND[side], INDEX1[side]);
    if aim[7] == 0.0 || hand >= g_VRFKSnapCount || index >= g_VRFKSnapCount {
        return None;
    }
    let (pos, rot) = (&*(&raw const g_VRFKSnapPos), &*(&raw const g_VRFKSnapRot));
    let hand_q = rot[hand as usize];
    let (ip, hp) = (pos[index as usize], pos[hand as usize]);
    let rel = [ip[0] - hp[0], ip[1] - hp[1], ip[2] - hp[2]];
    let mut hand_inv = [0.0f32; 4];
    VRIK_QuatConj(hand_q.as_ptr(), hand_inv.as_mut_ptr());
    let local = rotate(&hand_inv, &rel);
    let mut q = [aim[3], aim[4], aim[5], aim[6]];
    VRIK_QuatNorm(q.as_mut_ptr());
    let o = rotate(&q, &local);
    let model = [aim[0] + o[0], aim[1] + o[1], aim[2] + o[2]];
    let origin = model_to_world(m2w, &model);
    let a = rotate(&q, along);
    Some((origin, rotate(m2w, &a)))
}

unsafe fn finish(frame: *mut StackFrame) {
    if let Some(frame) = frame.as_mut() {
        frame.finish_params();
    }
}
macro_rules! params {
    ($frame:expr $(, $value:expr)*) => {
        if let Some(frame) = $frame.as_mut() {
            $(frame.get_parameter(&mut $value);)*
            frame.finish_params();
        }
    };
}
unsafe fn put<T>(out: *mut T, value: T) {
    if !out.is_null() {
        *out = value;
    }
}
/// A CString parameter's text as a C string (empty when it is not text).
fn c_text(s: &CString) -> std::ffi::CString {
    std::ffi::CString::new(s.as_str()).unwrap_or_default()
}

/// Rendered index tip, including the last phalanx; false when tracking or the rig is unavailable.
///
/// # Safety
/// `world` points to three floats.
#[no_mangle]
pub unsafe extern "C" fn cvr_vrmenu_finger_world(side: i32, world: *mut f32) -> bool {
    let mut input = VrController::default();
    if !(0..=1).contains(&side)
        || !super::xr_calls::controller_state(&mut input)
        || !(if side != 0 {
            input.right_hand_valid
        } else {
            input.left_hand_valid
        })
    {
        return false;
    }
    let end = [if side != 0 { -TIP } else { TIP }, 0.0, 0.0];
    let Some(m2w) = model_to_world_frame() else {
        return false;
    };
    let Some(p) = bone_world(&m2w, INDEX3[side as usize], &end) else {
        return false;
    };
    core::ptr::copy_nonoverlapping(p.as_ptr(), world, 3);
    true
}
/// The menu's blink, refreshed by the wait transition; 0 once its Lua owner stopped updating it for 2 s.
#[no_mangle]
pub extern "C" fn cvr_vrmenu_blink_closure() -> f32 {
    // SAFETY: plain Win32 call.
    let now = unsafe { GetTickCount64() };
    if now.wrapping_sub(BLINK_UPDATED.load(Acquire)) > 2000 {
        return 0.0;
    }
    f32::from_bits(BLINK.load(Relaxed))
}
/// While the menu is open (or full-body tracking calibrates: each step is a pull of both triggers) the triggers, grips
/// and face buttons belong to it, not to the game.
#[no_mangle]
pub extern "C" fn cvr_vrmenu_captures_buttons() -> bool {
    // SAFETY: a C++ service reading a flag.
    (interface_on() && CAPTURE.load(Relaxed)) || unsafe { cvr_cpp_fbt_calibrating() }
}
/// The VR interface (xr_vr_interface): off, the menus, the wheel and the vendor and storage routes leave the game alone.
fn interface_on() -> bool {
    cvr_settings::lc!(xrVrInterface) != 0
}
/// VRInterfaceEnabled() -> Bool.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRInterfaceEnabled(_this: *mut c_void, frame: *mut StackFrame, out: *mut bool, _unused: i64) {
    finish(frame);
    put(out, interface_on());
}
/// The weapon wheel's script asked for the hold of Y within the last half second.
#[no_mangle]
pub extern "C" fn cvr_vrmenu_wheel_armed(now: u64) -> bool {
    let beat = WHEEL_BEAT.load(Relaxed);
    beat != 0 && now >= beat && now - beat < 500
}
#[no_mangle]
pub extern "C" fn cvr_vrmenu_publish_wheel_hold(pending: bool, held: bool) {
    WHEEL_HOLD.store(
        if held {
            2
        } else if pending {
            1
        } else {
            0
        },
        Relaxed,
    );
}

/// VRMenuGesture() -> array<Vector4>: empty, or once per swipe [0] (1, sequence, 0, 0), [1] where it started and [2]
/// where it ended, relative to the head in its heading frame (right, up, forward, 1) metres; or, once per L3 + R3 hold,
/// the single item (2, 0, 0, 0): open or close the menu in front of the head. Never in a braindance (the grips are its
/// LB and RB there; an L3 + R3 hold is dropped rather than kept).
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRMenuGesture(
    _this: *mut c_void,
    frame: *mut StackFrame,
    out: *mut DynArray<Vector4>,
    _unused: i64,
) {
    finish(frame);
    let Some(out) = out.as_mut() else {
        return;
    };
    out.clear();
    let mut guard = GESTURE.lock().unwrap_or_else(|p| p.into_inner());
    let g = guard.get_or_insert_with(|| Gesture {
        swipe: SwipeDetector::default(),
        last_sequence: 0,
    });
    if cvr_in_braindance() || !interface_on() {
        cvr_cpp_vrui_consume_menu_summon();
        g.swipe.clear();
        return;
    }
    if cvr_cpp_vrui_consume_menu_summon() {
        out.push(v4(2.0, 0.0, 0.0, 0.0));
        return;
    }
    let mut head = HeadPose::default();
    let mut hands = [HeadPose::default(); 2];
    let (mut sequence, mut stamp) = (0u64, 0u64);
    let mut input = VrController::default();
    if controls().0 == 0
        || !super::xr_calls::gesture_hand_frame(
            &mut head,
            hands.as_mut_ptr(),
            &mut sequence,
            &mut stamp,
            false,
        )
        || !hands[1].valid
        || !super::xr_calls::controller_state(&mut input)
        || !input.right_hand_valid
    {
        g.swipe.clear();
        return;
    }
    if sequence == g.last_sequence {
        return;
    }
    g.last_sequence = sequence;
    let head_q = head.ori;
    let p = rotate(&head_q, &hands[1].pos);
    let t = stamp as f64 * 1e-6;
    // The right hand's grip on something is not a swipe: the steering wheel (owned, or still easing onto it), a
    // ladder's rung, a thing in its grasp, a railing or a ledge it holds on to (the pull up is the same move down).
    let holding = (cvr_anim_wheel_control_state() & 1) != 0
        || f32::from_bits(g_wheelBlendRight.load(Relaxed)) > 0.0
        || cvr_ladder_active()
        || cvr_anim_grasp_holding(1)
        || super::physhands::grab::holds_on(1);
    let hand = [p[0] + head.pos[0], p[1] + head.pos[1], p[2] + head.pos[2]];
    // THE SWIPE'S MODIFIER, as the launcher's "VR menu call" picks it: the left thumbrest held (by default on a Quest
    // 3), the left stick held in (by default on every other headset), or none -- the swipe alone. Never the right
    // grip, which grabs things.
    use cvr_settings::launcher::{menu_call_effective, MENU_CALL_GESTURE, MENU_CALL_STICK, MENU_CALL_THUMBREST};
    let call = menu_call_effective();
    let modifier = match call {
        MENU_CALL_GESTURE => true,
        MENU_CALL_THUMBREST => cvr_input::modifier::thumbrest_held(GetTickCount64()),
        _ => cvr_input::modifier::LEFT_CLICK.load(Relaxed),
    };
    let Some((start, end)) = g.swipe.push(t, &hand, modifier, holding) else {
        return;
    };
    if call == MENU_CALL_STICK {
        // the stick held for the swipe: its release is no sprint
        cvr_input::modifier::SWIPE_USED_CLICK.store(true, Relaxed);
    }
    // The head's heading frame in tracking space (OpenXR: Y up, -Z ahead).
    let mut f = rotate(&head_q, &[0.0, 0.0, -1.0]);
    f[1] = 0.0;
    let n = (f[0] * f[0] + f[2] * f[2]).sqrt();
    if n < 1e-4 {
        return;
    }
    f[0] /= n;
    f[2] /= n;
    let r = [-f[2], 0.0, f[0]];
    let relative = |s: &SwipeSample| {
        let d = [
            s.p[0] - head.pos[0],
            s.p[1] - head.pos[1],
            s.p[2] - head.pos[2],
        ];
        v4(dot(&d, &r), d[1], dot(&d, &f), 1.0)
    };
    out.push(v4(1.0, (sequence & 0xff_ffff) as f32, 0.0, 0.0));
    out.push(relative(&start));
    out.push(relative(&end));
}

/// VRMenuPointer(center, right, up, size) -> array<Vector4>, the menu plane given in the world (unit right and up axes,
/// size.X x size.Y metres; its front faces right x up), for each hand (0 left, 1 right):
///   [4h+0] fingertip: (u, v, distance in front of the plane, valid)   u, v in 0..1 across / down the plane
///   [4h+1] ray:       (u, v, length to the plane, hits)               hits = 1 only when it meets the front
///   [4h+2] ray origin in the world (x, y, z, trigger 0..1)
///   [4h+3] ray hit point in the world (x, y, z, grip 0..1)
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRMenuPointer(
    _this: *mut c_void,
    frame: *mut StackFrame,
    out: *mut DynArray<Vector4>,
    _unused: i64,
) {
    let (mut center, mut right, mut up, mut size) = (
        Vector4::default(),
        Vector4::default(),
        Vector4::default(),
        Vector4::default(),
    );
    params!(frame, center, right, up, size);
    let Some(out) = out.as_mut() else {
        return;
    };
    out.clear();
    for v in pointer(center, right, up, size) {
        out.push(v);
    }
}
/// VRMenuPointer's work for the plugin's own callers (Vector4s as [x, y, z, w]).
///
/// # Safety
/// The frame's skeleton and the controllers' reads.
pub unsafe fn menu_pointer(center: [f32; 4], right: [f32; 4], up: [f32; 4], size: [f32; 4]) -> Vec<[f32; 4]> {
    let v = |a: [f32; 4]| v4(a[0], a[1], a[2], a[3]);
    pointer(v(center), v(right), v(up), v(size)).into_iter().map(|p| [p.x, p.y, p.z, p.w]).collect()
}
unsafe fn pointer(center: Vector4, right: Vector4, up: Vector4, size: Vector4) -> Vec<Vector4> {
    let mut out = Vec::with_capacity(8);
    let (c, rx, uy) = (
        [center.x, center.y, center.z],
        [right.x, right.y, right.z],
        [up.x, up.y, up.z],
    );
    let nz = [
        rx[1] * uy[2] - rx[2] * uy[1],
        rx[2] * uy[0] - rx[0] * uy[2],
        rx[0] * uy[1] - rx[1] * uy[0],
    ];
    let w = if size.x > 0.01 { size.x } else { 1.0 };
    let h = if size.y > 0.01 { size.y } else { 1.0 };
    let mut input = VrController::default();
    // Raw: the native overlay's capture must not take the menu's own trigger away.
    let have_input = cvr_cpp_xr_raw_controller_state(&mut input);
    let m2w = model_to_world_frame().filter(|_| g_VRFKSnapCount > INDEX3[1]);
    for side in 0..2usize {
        let (mut tip, mut ray, mut origin, mut hit) = (
            v4(-1.0, -1.0, 0.0, 0.0),
            v4(-1.0, -1.0, 0.0, 0.0),
            Vector4::default(),
            Vector4::default(),
        );
        let trigger = if have_input {
            if side != 0 {
                input.right_trigger
            } else {
                input.left_trigger
            }
        } else {
            0.0
        };
        let grip = if have_input {
            if side != 0 {
                input.right_grip
            } else {
                input.left_grip
            }
        } else {
            0.0
        };
        let along = [if side != 0 { -1.0f32 } else { 1.0 }, 0.0, 0.0];
        let end = [along[0] * TIP, 0.0, 0.0];
        if let Some(m2w) = m2w.as_ref() {
            if let Some(p) = bone_world(m2w, INDEX3[side], &end) {
                let l = [p[0] - c[0], p[1] - c[1], p[2] - c[2]];
                tip = v4(
                    dot(&l, &rx) / w + 0.5,
                    0.5 - dot(&l, &uy) / h,
                    dot(&l, &nz),
                    1.0,
                );
            }
            if let Some((o, d)) = aim_world(m2w, side, &along) {
                origin = v4(o[0], o[1], o[2], trigger);
                let l = [o[0] - c[0], o[1] - c[1], o[2] - c[2]];
                let (front, into) = (dot(&l, &nz), dot(&d, &nz));
                if front > 0.0 && into < -1e-3 {
                    let t = -front / into;
                    let q = [o[0] + d[0] * t, o[1] + d[1] * t, o[2] + d[2] * t];
                    let lq = [q[0] - c[0], q[1] - c[1], q[2] - c[2]];
                    ray = v4(
                        dot(&lq, &rx) / w + 0.5,
                        0.5 - dot(&lq, &uy) / h,
                        t,
                        if t < 6.0 { 1.0 } else { 0.0 },
                    );
                    hit = v4(q[0], q[1], q[2], grip);
                } else {
                    hit = v4(
                        o[0] + d[0] * 2.0,
                        o[1] + d[1] * 2.0,
                        o[2] + d[2] * 2.0,
                        grip,
                    );
                }
            }
        }
        out.push(tip);
        out.push(ray);
        out.push(origin);
        out.push(hit);
    }
    out
}

/// VRMenuOpenSettings(): the port's own settings panel, as F10 opens it.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRMenuOpenSettings(
    _this: *mut c_void,
    frame: *mut StackFrame,
    out: *mut bool,
    _unused: i64,
) {
    finish(frame);
    let open = !cvr_cpp_vrui_visible();
    if open {
        cvr_cpp_vrui_toggle();
    }
    put(out, open);
}
/// VRGripFromFingers() -> Bool: the grip is read from the fingers (xr_grip_from_fingers).
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRGripFromFingers(
    _this: *mut c_void,
    frame: *mut StackFrame,
    out: *mut bool,
    _unused: i64,
) {
    finish(frame);
    put(out, cvr_fingergrip_grip_from_fingers());
}
/// VRMenuCapture(open): while the menu is open the triggers, grips and face buttons stop reaching the game.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRMenuCapture(
    _this: *mut c_void,
    frame: *mut StackFrame,
    out: *mut bool,
    _unused: i64,
) {
    let mut open = false;
    params!(frame, open);
    CAPTURE.store(open, Relaxed);
    put(out, open);
}
/// VRWeaponWheel(armed) -> Int32: called every frame by the port's weapon wheel. Armed, it takes the hold of Y from
/// the game for the next half second; the answer is the hold as the input merge last decided it (0 none, 1 pressed
/// and undecided, 2 held). Always 0 with xr_vr_weapon_wheel off.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRWeaponWheel(
    _this: *mut c_void,
    frame: *mut StackFrame,
    out: *mut i32,
    _unused: i64,
) {
    let mut armed = false;
    params!(frame, armed);
    let on = armed && controls().1 != 0;
    WHEEL_BEAT.store(if on { GetTickCount64() } else { 0 }, Relaxed);
    put(out, if on { WHEEL_HOLD.load(Relaxed) } else { 0 });
}
/// VRMenuBlink(closure) -> Bool: the menu's blink, 0..1.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRMenuBlink(
    _this: *mut c_void,
    frame: *mut StackFrame,
    out: *mut bool,
    _unused: i64,
) {
    let mut closure = 0.0f32;
    params!(frame, closure);
    let value = if closure.is_finite() {
        closure.clamp(0.0, 1.0)
    } else {
        0.0
    };
    BLINK.store(value.to_bits(), Relaxed);
    BLINK_UPDATED.store(GetTickCount64(), Release);
    put(out, true);
}
/// VRSettingsPage(page) -> String: the port's settings page as the VR menu draws it, the latest one recorded ("" until
/// the first), and a request for a fresh one, recorded on the next present.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRSettingsPage(
    _this: *mut c_void,
    frame: *mut StackFrame,
    out: *mut CString,
    _unused: i64,
) {
    let mut page = 0i32;
    params!(frame, page);
    cvr_cpp_uirec_request(page);
    if out.is_null() {
        return;
    }
    let mut text = vec![0u8; 4096];
    loop {
        let n = cvr_cpp_uirec_latest(page, text.as_mut_ptr(), text.len());
        if n <= text.len() {
            text.truncate(n);
            break;
        }
        text = vec![0u8; n];
    }
    CString::assign(out, &String::from_utf8_lossy(&text));
}
/// VRSettingsEdit(page, id, value): what the VR menu did to the item `id` of that page, applied through the panel code
/// on the next present.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRSettingsEdit(
    _this: *mut c_void,
    frame: *mut StackFrame,
    out: *mut bool,
    _unused: i64,
) {
    let (mut page, mut id, mut value) = (0i32, CString::empty(), 0.0f32);
    params!(frame, page, id, value);
    cvr_cpp_uirec_queue_edit(page, c_text(&id).as_ptr(), value as f64);
    put(out, true);
}
/// VRMenuPlayWav(file, volume) -> Bool: a sound file played once (relative to the plugin's Lua mods folder); false
/// when the file or the audio device is not there.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRMenuPlayWav(
    _this: *mut c_void,
    frame: *mut StackFrame,
    out: *mut bool,
    _unused: i64,
) {
    let (mut file, mut volume) = (CString::empty(), 1.0f32);
    params!(frame, file, volume);
    let played = cvr_sound_play_once(c_text(&file).as_ptr(), volume);
    put(out, played);
}
/// VRSoundChannel(channel, file, volume, lowpass, loopFrom, loopTo, crossfade, fade) -> Bool: what the named channel
/// plays now, reached over `fade` seconds; file "" ends it.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRSoundChannel(
    _this: *mut c_void,
    frame: *mut StackFrame,
    out: *mut bool,
    _unused: i64,
) {
    let (mut channel, mut file) = (CString::empty(), CString::empty());
    let (mut volume, mut lowpass, mut loop_from, mut loop_to, mut crossfade, mut fade) =
        (1.0f32, 0.0f32, -1.0f32, 0.0f32, 0.0f32, 0.0f32);
    params!(frame, channel, file, volume, lowpass, loop_from, loop_to, crossfade, fade);
    let name = c_text(&channel);
    let path = c_text(&file);
    let sound = Sound {
        file: path.as_ptr(),
        volume,
        lowpass,
        loop_from,
        loop_to,
        crossfade,
    };
    let ok = cvr_sound_channel(
        name.as_ptr(),
        &sound,
        if fade.is_finite() { fade } else { 0.0 },
    );
    put(out, ok);
}
