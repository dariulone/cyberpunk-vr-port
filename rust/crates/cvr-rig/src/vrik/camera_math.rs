//! FOV, IPD, and turning a quaternion into the basis the engine's camera wants (formerly src/Camera/CameraMath.cpp).
//! Every relationship here was measured, not taken from a book:
//!
//!   * The half IPD is half because the two eyes are placed symmetrically about the camera position.
//!   * WriteRenderCameraBasis and ApplyFinalCameraOrientationFromQuat write ROWS, in an order established by writing
//!     a known rotation and reading back what moved (CyberpunkVR_CamFinalRowOrder, below).
//!
//! The engine memory is written through the C++ half's guarded copies (src/Utils/MemorySafeC.cpp): a freed camera
//! must cost one frame's write, not the game.
use super::env::{cvr_cpp_read_floats_safe, cvr_cpp_write_floats_safe};

extern "C" {
    static CyberpunkVR_CamFinalRowOrder: i32;
    // src/Camera/CameraServices.cpp
    fn cvr_cpp_camera_scales(out: *mut f32);
    fn cvr_cpp_xr_runtime_ipd() -> f32;
}

/// g_liveControls' world, IPD and stereo scales, in that order.
fn scales() -> [f32; 3] {
    let mut s = [0.0f32; 3];
    // SAFETY: writes three floats.
    unsafe { cvr_cpp_camera_scales(s.as_mut_ptr()) };
    s
}

/// Uniform world scale: it multiplies both the eye separation and the head-translation gain, so lowering it makes
/// the world appear bigger.
#[no_mangle]
pub extern "C" fn cvr_camera_world_scale() -> f32 {
    let ws = scales()[0];
    if ws > 0.0 {
        ws
    } else {
        1.0
    }
}

/// The half IPD straight from the OpenXR runtime's view separation (it adapts to whoever wears the HMD), times the
/// IPD, world and stereo scales; the neutral baseline is 1:1 with the runtime.
#[no_mangle]
pub extern "C" fn cvr_camera_desired_half_ipd() -> f32 {
    // SAFETY: a plain read of the runtime's last view separation.
    let runtime_ipd = unsafe { cvr_cpp_xr_runtime_ipd() };
    let half_ipd = if runtime_ipd > 0.001 { runtime_ipd * 0.5 } else { 0.032 };
    let [_, mut ipd_scale, mut stereo_scale] = scales();
    if !(ipd_scale > 0.0) {
        ipd_scale = 1.0; // the zero-init window / bad values: honest runtime IPD
    }
    if !(stereo_scale > 0.0) {
        stereo_scale = 1.0;
    }
    if half_ipd > 0.0001 {
        half_ipd * cvr_camera_world_scale() * ipd_scale * stereo_scale
    } else {
        0.0
    }
}

/// The overlay's half IPD (it lives in this DLL too).
#[no_mangle]
pub extern "C" fn CyberpunkVRPort_HalfIpd() -> f32 {
    cvr_camera_desired_half_ipd()
}

#[no_mangle]
pub unsafe extern "C" fn cvr_camera_plausible_unit_vector3(v: *const f32) -> bool {
    if v.is_null() {
        return false;
    }
    let v = core::slice::from_raw_parts(v, 3);
    if !v.iter().all(|x| x.is_finite()) {
        return false;
    }
    let len_sq = v[0] * v[0] + v[1] * v[1] + v[2] * v[2];
    len_sq > 0.25 && len_sq < 4.0
}
#[no_mangle]
pub unsafe extern "C" fn cvr_camera_plausible_unit_quaternion(q: *const f32) -> bool {
    if q.is_null() {
        return false;
    }
    let q = core::slice::from_raw_parts(q, 4);
    if !q.iter().all(|x| x.is_finite()) {
        return false;
    }
    let len_sq = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
    len_sq > 0.25 && len_sq < 4.0
}
#[no_mangle]
pub unsafe extern "C" fn cvr_camera_plausible_position_vec4(v: *const f32) -> bool {
    if v.is_null() {
        return false;
    }
    let v = core::slice::from_raw_parts(v, 4);
    if !v.iter().all(|x| x.is_finite()) {
        return false;
    }
    (v[3] - 1.0).abs() < 0.25
}

#[no_mangle]
pub unsafe extern "C" fn cvr_camera_right_vector_from_quaternion(q: *const f32, out_right: *mut f32) {
    if q.is_null() || out_right.is_null() {
        return;
    }
    let (x, y, z, w) = (*q, *q.add(1), *q.add(2), *q.add(3));
    *out_right = 1.0 - 2.0 * (y * y + z * z);
    *out_right.add(1) = 2.0 * (x * y + z * w);
    *out_right.add(2) = 2.0 * (x * z - y * w);
}

/// The axes as rows: [0..3] right, [4..7] forward, [8..11] up.
fn game_view_rows(q: &[f32; 4]) -> [f32; 12] {
    let [x, y, z, w] = *q;
    [
        1.0 - 2.0 * (y * y + z * z),
        2.0 * (x * y + z * w),
        2.0 * (x * z - y * w),
        0.0,
        2.0 * (x * z + y * w),
        2.0 * (y * z - x * w),
        1.0 - 2.0 * (x * x + y * y),
        0.0,
        2.0 * (x * y - z * w),
        1.0 - 2.0 * (x * x + z * z),
        2.0 * (y * z + x * w),
        0.0,
    ]
}

/// The orientation into the RENDER CAMERA's own basis rows, the ones the view-matrix bake reads (component
/// +0xC0/+0xD0/+0xE0: rsiPtr + 20/24/28 floats, rsiPtr = component+0x70), and its quaternion at +0x80.
///
/// CyberpunkVR_CamFinalRowOrder 1 = transposed, MEASURED: for the same engine camera the quaternion at rsiPtr+4 and
/// the rows at rsiPtr+20 agree as row[r] = (R[r][0], R[r][2], R[r][1]) -- the game's X right, Y forward, Z up
/// against the quaternion's X right, Y up, Z forward -- which is the transpose of game_view_rows. 0 keeps the
/// straight form for comparison. The 4th lane of each row is the engine's and is left alone.
#[no_mangle]
pub unsafe extern "C" fn cvr_camera_write_render_camera_basis(rsi: *mut f32, q: *const f32) {
    if rsi.is_null() || q.is_null() {
        return;
    }
    let qv = [*q, *q.add(1), *q.add(2), *q.add(3)];
    let v = game_view_rows(&qv);
    let rows = if CyberpunkVR_CamFinalRowOrder == 0 {
        v
    } else {
        [v[0], v[4], v[8], 0.0, v[1], v[5], v[9], 0.0, v[2], v[6], v[10], 0.0]
    };
    for r in 0..3 {
        cvr_cpp_write_floats_safe(rsi.add(20 + r * 4), rows.as_ptr().add(r * 4), 3);
    }
    cvr_cpp_write_floats_safe(rsi.add(4), q, 4);
}

/// The older three-target write: the quaternion (+4), the camera matrix's rotation block (+20, the transpose of the
/// rows -- harmless on the near-identity cant correction the AER path applies), the view rows (+68) and the view
/// packet (+204: the rows scaled as they were, and the view position from the 17-bit fixed-point world position at
/// rsiPtr, times 50/131072).
#[no_mangle]
pub unsafe extern "C" fn cvr_camera_apply_final_orientation_from_quat(rsi: *mut f32, q: *const f32) {
    if rsi.is_null() || q.is_null() || !cvr_camera_plausible_unit_quaternion(q) {
        return;
    }
    let qv = [*q, *q.add(1), *q.add(2), *q.add(3)];
    let v = game_view_rows(&qv);
    // Keep the raw quaternion in sync so any downstream camera rebuild sees the VR orientation.
    cvr_cpp_write_floats_safe(rsi.add(4), q, 4);
    let mut m = [0.0f32; 16];
    if cvr_cpp_read_floats_safe(rsi.add(20), m.as_mut_ptr(), 16) {
        m[0] = v[0];
        m[1] = v[4];
        m[2] = v[8];
        m[4] = v[1];
        m[5] = v[5];
        m[6] = v[9];
        m[8] = v[2];
        m[9] = v[6];
        m[10] = v[10];
        cvr_cpp_write_floats_safe(rsi.add(20), m.as_ptr(), 16);
    }
    cvr_cpp_write_floats_safe(rsi.add(68), v.as_ptr(), 12);
    let mut p = [0.0f32; 16];
    if cvr_cpp_read_floats_safe(rsi.add(204), p.as_mut_ptr(), 16) {
        let mut scale0 = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
        let mut scale1 = (p[4] * p[4] + p[5] * p[5] + p[6] * p[6]).sqrt();
        if !scale0.is_finite() || scale0 < 0.05 || scale0 > 10.0 {
            scale0 = 1.0;
        }
        if !scale1.is_finite() || scale1 < 0.05 || scale1 > 10.0 {
            scale1 = scale0;
        }
        p[0] = v[0] * scale0;
        p[1] = v[1] * scale0;
        p[2] = v[2] * scale0;
        p[4] = v[4] * scale1;
        p[5] = v[5] * scale1;
        p[6] = v[6] * scale1;
        p[12] = v[8];
        p[13] = v[9];
        p[14] = v[10];
        // The ABSOLUTE WorldPosition at the true 131072 (17-bit) fixed-point scale; 50/131072 keeps that scale
        // explicit (bit-identical to the old 25/65536): world metres * 50 for the +0x330 view-position row.
        let fp = rsi as *const i32;
        let scale_view = 50.0f32 / 131072.0;
        p[8] = *fp as f32 * scale_view;
        p[9] = *fp.add(1) as f32 * scale_view;
        p[10] = *fp.add(2) as f32 * scale_view;
        cvr_cpp_write_floats_safe(rsi.add(204), p.as_ptr(), 16);
    }
}
