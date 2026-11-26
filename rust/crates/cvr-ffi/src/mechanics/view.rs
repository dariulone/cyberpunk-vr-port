//! What the Stereo mod's measuring modules shared: the active camera (CameraSystem's world transform and forward axis),
//! the world raycasts (SpatialQueriesSystem, the hit's TraceResult through the out parameter) and the f64 vector math
//! Lua's numbers did.
use crate::script_call::{method, Value};
use core::ffi::c_void;
use red4ext_abi::cname::CName;

pub(super) type V3 = [f64; 3];

pub(super) fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
pub(super) fn norm(a: V3) -> Option<V3> {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    (l >= 1e-6).then(|| [a[0] / l, a[1] / l, a[2] / l])
}
/// Vector4.new(a.x, a.y, a.z, w).
pub(super) fn v4(a: V3, w: f32) -> [f32; 4] {
    [a[0] as f32, a[1] as f32, a[2] as f32, w]
}
/// A Vector4's x, y, z as Lua read them.
pub(super) fn v3(a: [f32; 4]) -> V3 {
    [a[0] as f64, a[1] as f64, a[2] as f64]
}
/// `p`'s depth from `o` along the axis `f`.
pub(super) fn depth(p: V3, o: V3, f: V3) -> f64 {
    (p[0] - o[0]) * f[0] + (p[1] - o[1]) * f[1] + (p[2] - o[2]) * f[2]
}

/// physicsTraceResult (0x60 bytes, 16-aligned; position, a Vector3, first): a plain value, nothing to destroy.
#[repr(C, align(16))]
struct TraceResult([f32; 24]);
/// Transform: position (Vector4), orientation (Quaternion).
#[repr(C, align(16))]
struct Transform([f32; 8]);

/// Game.GetCameraSystem(): the active camera's position (GetActiveCameraWorldTransform's out transform) and forward
/// axis (GetActiveCameraForward); None without the camera system.
pub(super) fn camera() -> Result<Option<(V3, [f32; 4])>, String> {
    let cam_held = super::game_system("GetCameraSystem");
    let cam = cam_held.instance;
    if cam.is_null() {
        return Ok(None);
    }
    let mut t = Transform([0.0; 8]);
    // SAFETY: the camera system; the transform's storage outlives the call.
    unsafe { method::<bool>(cam, "GetActiveCameraWorldTransform", &[Value::Out((&mut t as *mut Transform).cast())]) }
        .ok_or("GetActiveCameraWorldTransform failed")?;
    // SAFETY: as above.
    let f = unsafe { method::<[f32; 4]>(cam, "GetActiveCameraForward", &[]) }.ok_or("GetActiveCameraForward failed")?;
    Ok(Some(([t.0[0] as f64, t.0[1] as f64, t.0[2] as f64], f)))
}

/// `sq:<function>(Vector4(a, 1), Vector4(b, 1), group, out result, flags...)` -- SyncRaycastByQueryPreset or
/// SyncRaycastByCollisionGroup: the hit's position, None when nothing was hit.
pub(super) fn raycast(sq: *mut c_void, function: &str, a: V3, b: V3, group: &str, flags: &[Value]) -> Result<Option<V3>, String> {
    let (a, b) = (v4(a, 1.0), v4(b, 1.0));
    let mut trace = TraceResult([0.0; 24]);
    let mut values = [Value::Bool(false); 6];
    values[..4].copy_from_slice(&[
        Value::Ptr(a.as_ptr().cast()),
        Value::Ptr(b.as_ptr().cast()),
        Value::U64(CName::hashed(group).0),
        Value::Out((&mut trace as *mut TraceResult).cast()),
    ]);
    let n = 4 + flags.len().min(2);
    values[4..n].copy_from_slice(&flags[..n - 4]);
    // SAFETY: the spatial queries system; the trace result's storage outlives the call.
    let hit = unsafe { method::<bool>(sq, function, &values[..n]) }.ok_or_else(|| format!("{function} failed"))?;
    Ok(hit.then(|| [trace.0[0] as f64, trace.0[1] as f64, trace.0[2] as f64]))
}
