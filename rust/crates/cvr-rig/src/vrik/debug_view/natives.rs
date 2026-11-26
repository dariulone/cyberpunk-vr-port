//! The collision view's natives and the gun's (main's DebugView.cpp: VRDebugOn, VRDebugViewSet, VRDebugLine, VRDebugRay,
//! VRDebugCapsule, VRDebugPoint, VRDebugWorld, VRDebugProbeBody, VRBodyFilterInfo, VRBodyPhysXPos, VRGunBoxClear,
//! VRGunBoxAdd, VRGunAnchor, VRGunPush, VRGunTouches, VRGunTouchOf, VRGunTouchWay, VRProxyBodies, VRThingOutline,
//! VRDebugFlush).
use super::view::view;
use super::*;
use red4ext_abi::stack::StackFrame;

type Frame = StackFrame;
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
fn v3(v: &[f32; 4]) -> Vec3 {
    [v[0], v[1], v[2]]
}

#[no_mangle]
pub unsafe extern "C" fn VRDebugOn(_this: *mut c_void, frame: *mut Frame, out: *mut bool, _: i64) {
    params!(frame);
    put(out, super::view::on());
}
/// VRDebugViewSet(on) -> Bool: the collision view on or off, as the overlay's checkbox (a test tool's way to it).
#[no_mangle]
pub unsafe extern "C" fn VRDebugViewSet(_this: *mut c_void, frame: *mut Frame, out: *mut bool, _: i64) {
    let mut on = false;
    params!(frame, on);
    cvr_settings::lc_set!(xrDebugView, i32::from(on));
    put(out, on);
}
#[no_mangle]
pub unsafe extern "C" fn VRDebugLine(_this: *mut c_void, frame: *mut Frame, out: *mut bool, _: i64) {
    let (mut a, mut b, mut color) = ([0.0f32; 4], [0.0f32; 4], 0i32);
    params!(frame, a, b, color);
    view().add(&v3(&a), &v3(&b), color);
    put(out, true);
}
#[no_mangle]
pub unsafe extern "C" fn VRDebugRay(_this: *mut c_void, frame: *mut Frame, out: *mut bool, _: i64) {
    let (mut a, mut b, mut color) = ([0.0f32; 4], [0.0f32; 4], 0i32);
    params!(frame, a, b, color);
    view().hold_ray(&v3(&a), &v3(&b), color);
    put(out, true);
}
/// An orthonormal frame round the way `v`: `a` along it, `b` and `c` across.
fn frame_of(v: &Vec3) -> (Vec3, Vec3, Vec3) {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let a = if len < 1e-5 { [0.0, 0.0, 1.0] } else { [v[0] / len, v[1] / len, v[2] / len] };
    let h = [if a[2].abs() < 0.9 { 0.0 } else { 1.0 }, 0.0, if a[2].abs() < 0.9 { 1.0 } else { 0.0 }];
    let mut b = [a[1] * h[2] - a[2] * h[1], a[2] * h[0] - a[0] * h[2], a[0] * h[1] - a[1] * h[0]];
    let bl = (b[0] * b[0] + b[1] * b[1] + b[2] * b[2]).sqrt();
    b = [b[0] / bl, b[1] / bl, b[2] / bl];
    let c = [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    (a, b, c)
}
fn world_ring(st: &mut super::view::ViewState, center: &Vec3, u: &Vec3, v: &Vec3, radius: f32, color: i32, from: f32, to: f32, parts: i32) {
    let mut last = [0.0f32; 3];
    for i in 0..=parts {
        let angle = from + (to - from) * i as f32 / parts as f32;
        let (c, s) = (angle.cos() * radius, angle.sin() * radius);
        let p = [center[0] + u[0] * c + v[0] * s, center[1] + u[1] * c + v[1] * s, center[2] + u[2] * c + v[2] * s];
        if i != 0 {
            st.add(&last, &p, color);
        }
        last = p;
    }
}
#[no_mangle]
pub unsafe extern "C" fn VRDebugCapsule(_this: *mut c_void, frame: *mut Frame, out: *mut bool, _: i64) {
    let (mut a, mut b, mut radius, mut color) = ([0.0f32; 4], [0.0f32; 4], 0.0f32, 0i32);
    params!(frame, a, b, radius, color);
    let pi = core::f32::consts::PI;
    let (pa, pb) = (v3(&a), v3(&b));
    let d = [pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]];
    let (x, y, z) = frame_of(&d);
    let mut st = view();
    world_ring(&mut st, &pa, &y, &z, radius, color, 0.0, 2.0 * pi, 8);
    world_ring(&mut st, &pb, &y, &z, radius, color, 0.0, 2.0 * pi, 8);
    for side in 0..4 {
        let axis = if side < 2 { y } else { z };
        let sign = if side % 2 != 0 { -radius } else { radius };
        let s = [pa[0] + axis[0] * sign, pa[1] + axis[1] * sign, pa[2] + axis[2] * sign];
        let e = [pb[0] + axis[0] * sign, pb[1] + axis[1] * sign, pb[2] + axis[2] * sign];
        st.add(&s, &e, color);
    }
    world_ring(&mut st, &pb, &y, &x, radius, color, 0.0, pi, 4);
    world_ring(&mut st, &pb, &z, &x, radius, color, 0.0, pi, 4);
    world_ring(&mut st, &pa, &y, &x, -radius, color, 0.0, pi, 4);
    world_ring(&mut st, &pa, &z, &x, -radius, color, 0.0, pi, 4);
    put(out, true);
}
#[no_mangle]
pub unsafe extern "C" fn VRDebugPoint(_this: *mut c_void, frame: *mut Frame, out: *mut bool, _: i64) {
    let (mut p, mut size, mut color) = ([0.0f32; 4], 0.01f32, 0i32);
    params!(frame, p, size, color);
    let mut st = view();
    for axis in 0..3 {
        let (mut s, mut e) = (v3(&p), v3(&p));
        s[axis] -= size;
        e[axis] += size;
        st.add(&s, &e, color);
    }
    put(out, true);
}
/// VRDebugWorld(center, radius) -> Int32: the real collision shapes within radius of center, in the game's world, into
/// the view: how many, or -1 when they are out of reach.
#[no_mangle]
pub unsafe extern "C" fn VRDebugWorld(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _: i64) {
    let (mut center, mut radius) = ([0.0f32; 4], 2.0f32);
    params!(frame, center, radius);
    put(out, super::view::world(&v3(&center), radius));
}
static PROBES: core::sync::atomic::AtomicI32 = core::sync::atomic::AtomicI32::new(0);
/// VRDebugProbeBody(proxy, body, world) -> Bool: what a body in the hand is, for the log. A few times a session.
#[no_mangle]
pub unsafe extern "C" fn VRDebugProbeBody(_this: *mut c_void, frame: *mut Frame, out: *mut bool, _: i64) {
    let (mut proxy, mut body, mut world) = (0i32, 0i32, [0.0f32; 4]);
    params!(frame, proxy, body, world);
    if PROBES.fetch_add(1, Relaxed) < 6 {
        super::pick::probe_proxy(proxy as u32);
        super::pick::probe_body(proxy as u32, body as u32, &v3(&world));
    }
    put(out, true);
}
/// VRBodyFilterInfo(proxy, body) -> Bool: what decides the body's contacts, to the log.
#[no_mangle]
pub unsafe extern "C" fn VRBodyFilterInfo(_this: *mut c_void, frame: *mut Frame, out: *mut bool, _: i64) {
    let (mut proxy, mut body) = (0i32, 0i32);
    params!(frame, proxy, body);
    if proxy > 0 && body >= 0 {
        super::pick::filter_info(proxy as u32, body as u32);
    }
    put(out, true);
}
/// VRBodyPhysXPos(proxy, body) -> Vector4: where PhysX has a body, in the world, w 1; w 0 if it has none.
#[no_mangle]
pub unsafe extern "C" fn VRBodyPhysXPos(_this: *mut c_void, frame: *mut Frame, out: *mut [f32; 4], _: i64) {
    let (mut proxy, mut body) = (0i32, 0i32);
    params!(frame, proxy, body);
    let v = super::pick::actor_world(proxy as u32, body as u32).map_or([0.0; 4], |p| [p[0], p[1], p[2], 1.0]);
    put(out, v);
}
#[no_mangle]
pub unsafe extern "C" fn VRGunBoxClear(_this: *mut c_void, frame: *mut Frame, out: *mut bool, _: i64) {
    params!(frame);
    super::gun::box_clear();
    put(out, true);
}
#[no_mangle]
pub unsafe extern "C" fn VRGunBoxAdd(_this: *mut c_void, frame: *mut Frame, out: *mut bool, _: i64) {
    let (mut lo, mut hi) = ([0.0f32; 4], [0.0f32; 4]);
    params!(frame, lo, hi);
    put(out, super::gun::box_add(&v3(&lo), &v3(&hi)));
}
#[no_mangle]
pub unsafe extern "C" fn VRGunAnchor(_this: *mut c_void, frame: *mut Frame, out: *mut bool, _: i64) {
    let mut at = [0.0f32; 4];
    params!(frame, at);
    super::gun::anchor(&v3(&at));
    put(out, true);
}
#[no_mangle]
pub unsafe extern "C" fn VRGunPush(_this: *mut c_void, frame: *mut Frame, out: *mut [f32; 4], _: i64) {
    let (mut at, mut turn) = ([0.0f32; 4], [0.0f32; 4]);
    params!(frame, at, turn);
    if out.is_null() {
        return;
    }
    *out = super::gun::push(&v3(&at), &turn);
}
#[no_mangle]
pub unsafe extern "C" fn VRGunTouches(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _: i64) {
    let (mut at, mut turn) = ([0.0f32; 4], [0.0f32; 4]);
    params!(frame, at, turn);
    put(out, super::gun::touches(&v3(&at), &turn));
}
#[no_mangle]
pub unsafe extern "C" fn VRGunTouchOf(_this: *mut c_void, frame: *mut Frame, out: *mut [f32; 4], _: i64) {
    let mut i = 0i32;
    params!(frame, i);
    put(out, super::gun::touch_of(i).map_or([0.0; 4], |t| [t.proxy as f32, t.body as f32, 0.0, 1.0]));
}
#[no_mangle]
pub unsafe extern "C" fn VRGunTouchWay(_this: *mut c_void, frame: *mut Frame, out: *mut [f32; 4], _: i64) {
    let mut i = 0i32;
    params!(frame, i);
    put(out, super::gun::touch_of(i).map_or([0.0; 4], |t| [t.away[0], t.away[1], t.away[2], t.depth]));
}
#[no_mangle]
pub unsafe extern "C" fn VRProxyBodies(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _: i64) {
    let mut proxy = 0i32;
    params!(frame, proxy);
    put(out, super::pick::proxy_bodies(proxy as u32));
}
/// VRThingOutline(proxy, body, colour): the body's real shape, in the view's lines. The caller flushes, whether the view
/// is on or not.
#[no_mangle]
pub unsafe extern "C" fn VRThingOutline(_this: *mut c_void, frame: *mut Frame, out: *mut bool, _: i64) {
    let (mut proxy, mut body, mut color) = (0i32, 0i32, 0i32);
    params!(frame, proxy, body, color);
    put(out, super::view::outline(proxy as u32, body as u32, color));
}
#[no_mangle]
pub unsafe extern "C" fn VRDebugFlush(_this: *mut c_void, frame: *mut Frame, out: *mut bool, _: i64) {
    params!(frame);
    view().flush();
    put(out, true);
}
