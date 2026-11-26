//! The grasp's natives (main's GraspPose.cpp; side 0 left, 1 right) and their C forms (cvr_grasp_*).
use super::seat::seat;
use super::*;
use cvr_host::log;

fn valid_request(side: i32, shape: i32) -> bool {
    (0..=1).contains(&side) && (K_BOX..=K_CONE).contains(&shape)
}
const KINDS: [&str; 8] = ["flat", "wrap", "ball", "pinch", "edge", "from above", "from below", "three fingers"];

/// VRGraspSeat: where a thing of that shape, at that pose in the slot when the hand takes it, lies for its grasp, in
/// the slot's frame. False (and nothing written) when the palm is not measured yet.
#[no_mangle]
pub unsafe extern "C" fn cvr_grasp_seat(side: i32, shape: i32, half: *const f32, at: *const f32, orientation: *const f32, outPos: *mut f32, outRot: *mut f32) -> bool {
    if !valid_request(side, shape) {
        return false;
    }
    let sd = side as usize;
    let h = a3(half);
    let up = up_in_hand(sd);
    let Some((s, kind)) = seat(sd, shape, &h, &a3(at), &a4(orientation), up.as_ref(), 15, false, None, 0) else {
        return false;
    };
    let how = debug_grasp_approach(sd);
    log!(
        "[grasp] {} hand seats shape {} (half {:.3} {:.3} {:.3}) for a {} grasp; a long thing: palm to its axis {:.2}, along it {:.2} of its half, that end to up {:.2}\n",
        if side != 0 { "right" } else { "left" },
        shape,
        h[0],
        h[1],
        h[2],
        KINDS[kind as usize],
        how[0],
        how[1],
        how[2]
    );
    put3(outPos, &s.p);
    put4(outRot, &s.q);
    true
}

/// VRGraspSet: the hand holds that thing there (slot frame); its fingers close on it from the next pass.
#[no_mangle]
pub unsafe extern "C" fn cvr_grasp_set(side: i32, shape: i32, half: *const f32, position: *const f32, orientation: *const f32) -> bool {
    if !valid_request(side, shape) {
        return false;
    }
    let mut st = lock(side as usize);
    // a baked grasp is only the hold grasp_take_baked began
    st.req.baked = false;
    st.req.shape = shape;
    st.req.half = a3(half);
    st.req.obj.p = a3(position);
    st.req.obj.q = a4(orientation);
    qnorm(&mut st.req.obj.q);
    st.req.force = -1;
    st.req.limit = 1.0;
    st.req.spread = -1.0;
    st.active = true;
    st.pin = true;
    st.rev = st.rev.wrapping_add(1);
    true
}

/// VRGraspPin: whether the slot is held at the rig's reference while this grasp lasts. A holster's weapon is not on the
/// slot while the fingers close on it, and the weapon itself comes onto the slot while they still are: pinned, the
/// game's animation cannot move the slot to that weapon's hold.
#[no_mangle]
pub extern "C" fn cvr_grasp_pin(side: i32, pinned: bool) -> bool {
    if !(0..=1).contains(&side) {
        return false;
    }
    lock(side as usize).pin = pinned;
    true
}

/// VRGraspBlend: how far the fingers are on the grasp (0 tracking, 1 the grasp).
#[no_mangle]
pub extern "C" fn cvr_grasp_blend(side: i32, blend: f32) -> bool {
    if !(0..=1).contains(&side) {
        return false;
    }
    lock(side as usize).blend = clamp(blend, 0.0, 1.0);
    true
}

/// VRGraspClear: the hand holds nothing; its fingers are the tracking's again.
#[no_mangle]
pub extern "C" fn cvr_grasp_clear(side: i32) -> bool {
    if !(0..=1).contains(&side) {
        return false;
    }
    let mut st = lock(side as usize);
    st.active = false;
    st.blend = 0.0;
    true
}

/// RED4ext::Vector4 / RED4ext::Quaternion (i, j, k, r) / RED4ext::Transform.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct ScriptVector4 {
    x: f32,
    y: f32,
    z: f32,
    w: f32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct ScriptQuaternion {
    i: f32,
    j: f32,
    k: f32,
    r: f32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ScriptTransform {
    position: ScriptVector4,
    orientation: ScriptQuaternion,
}
const _: () = assert!(core::mem::size_of::<ScriptTransform>() == 32);

type VmFrame = red4ext_abi::stack::StackFrame;

/// VRGraspSeat's and VRGraspSet's parameters: side, shape, the three half sizes, a position and an orientation.
struct GraspArgs {
    side: i32,
    shape: i32,
    half: [f32; 3],
    p: ScriptVector4,
    q: ScriptQuaternion,
}
unsafe fn read_grasp_args(frame: *mut VmFrame) -> GraspArgs {
    let mut a = GraspArgs { side: -1, shape: 0, half: [0.0; 3], p: ScriptVector4::default(), q: ScriptQuaternion::default() };
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut a.side);
        frame.get_parameter(&mut a.shape);
        for h in &mut a.half {
            frame.get_parameter(h);
        }
        frame.get_parameter(&mut a.p);
        frame.get_parameter(&mut a.q);
        frame.finish_params();
    }
    a
}

/// VRGraspSeat(side, shape, hx, hy, hz, position, orientation) -> Transform: where a thing of that shape, at that pose
/// in the slot when the hand takes it, lies for its grasp (slot frame); position.W = 1 when seated, 0 when the palm is
/// not measured yet.
#[no_mangle]
pub unsafe extern "C" fn VRGraspSeat(_this: *mut core::ffi::c_void, frame: *mut VmFrame, out: *mut ScriptTransform, _unused: i64) {
    let a = read_grasp_args(frame);
    let Some(out) = out.as_mut() else { return };
    out.position = ScriptVector4::default();
    out.orientation = a.q;
    let at = [a.p.x, a.p.y, a.p.z];
    let input = [a.q.i, a.q.j, a.q.k, a.q.r];
    let (mut position, mut rotation) = ([0.0f32; 3], [0.0f32; 4]);
    if !cvr_grasp_seat(a.side, a.shape, a.half.as_ptr(), at.as_ptr(), input.as_ptr(), position.as_mut_ptr(), rotation.as_mut_ptr()) {
        return;
    }
    out.position = ScriptVector4 { x: position[0], y: position[1], z: position[2], w: 1.0 };
    out.orientation = ScriptQuaternion { i: rotation[0], j: rotation[1], k: rotation[2], r: rotation[3] };
}
/// VRGraspSet(side, shape, hx, hy, hz, position, orientation) -> Bool: the hand holds that thing there (slot frame);
/// its fingers close on it from the next pass.
#[no_mangle]
pub unsafe extern "C" fn VRGraspSet(_this: *mut core::ffi::c_void, frame: *mut VmFrame, out: *mut bool, _unused: i64) {
    let a = read_grasp_args(frame);
    let at = [a.p.x, a.p.y, a.p.z];
    let input = [a.q.i, a.q.j, a.q.k, a.q.r];
    let ok = cvr_grasp_set(a.side, a.shape, a.half.as_ptr(), at.as_ptr(), input.as_ptr());
    if !out.is_null() {
        *out = ok;
    }
}
/// VRGraspPin(side, pinned) -> Bool: whether the slot is held at the rig's reference while this grasp lasts.
#[no_mangle]
pub unsafe extern "C" fn VRGraspPin(_this: *mut core::ffi::c_void, frame: *mut VmFrame, out: *mut bool, _unused: i64) {
    let (mut side, mut pinned) = (-1i32, true);
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut side);
        frame.get_parameter(&mut pinned);
        frame.finish_params();
    }
    let ok = cvr_grasp_pin(side, pinned);
    if !out.is_null() {
        *out = ok;
    }
}
/// VRGraspBlend(side, blend) -> Bool: how far the fingers are on the grasp (0 tracking, 1 the grasp).
#[no_mangle]
pub unsafe extern "C" fn VRGraspBlend(_this: *mut core::ffi::c_void, frame: *mut VmFrame, out: *mut bool, _unused: i64) {
    let (mut side, mut blend) = (-1i32, 0.0f32);
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut side);
        frame.get_parameter(&mut blend);
        frame.finish_params();
    }
    let ok = cvr_grasp_blend(side, blend);
    if !out.is_null() {
        *out = ok;
    }
}
/// VRGraspClear(side) -> Bool: the hand holds nothing; its fingers are the tracking's again.
#[no_mangle]
pub unsafe extern "C" fn VRGraspClear(_this: *mut core::ffi::c_void, frame: *mut VmFrame, out: *mut bool, _unused: i64) {
    let mut side = -1i32;
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut side);
        frame.finish_params();
    }
    let ok = cvr_grasp_clear(side);
    if !out.is_null() {
        *out = ok;
    }
}
