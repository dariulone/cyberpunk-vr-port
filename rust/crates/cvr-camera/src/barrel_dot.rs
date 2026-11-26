//! The barrel dots' arithmetic (src/Overlay/OverlayDebugDraw.cpp, `DrawBarrelCrosshair` and `DrawLeftBarrelDot`): the
//! located camera's right axis (column 0 of its rotation); the half IPD with its fallbacks (the shared slot, then
//! 32.5 mm); the located head centre from its 15.17 fixed-point publication (far enough from the origin to be real);
//! the left gun's muzzle forward normalised (long enough, the muzzle far enough from the origin); the ray from one eye
//! -- half an IPD along the right axis, MAIN's side by the eye sign -- to the point the bullet reaches at the dot
//! distance, in the camera's frame (overlay_projection's rotate by the inverse quaternion); MAIN's eye sign; the second
//! eye's parallax at the sight's zero distance; and NDC to and from a pixel in an eye's half of the backbuffer.
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_bd_*` (the projections and the drawing stay the .cpp's);
//! the unit tests below cover each. The operations keep the .cpp's order, so the results are bit for bit its own.

use crate::overlay_projection::rotate;

/// The camera's right axis.
pub fn camera_right(q: [f32; 4]) -> [f32; 3] {
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y + z * w), 2.0 * (x * z - y * w)]
}
/// The half IPD: the published one, else the shared slot's, else 32.5 mm.
pub fn half_ipd(published: f32, slot: f32) -> f32 {
    let mut h = published;
    if !(h > 0.0001) {
        h = slot;
    }
    if !(h > 0.0001) {
        h = 0.0325;
    }
    h
}
/// The head centre and whether it is real (more than a metre from the origin).
pub fn head_center(fixed: [i32; 3]) -> ([f32; 3], bool) {
    let h = fixed.map(|v| v as f32 / 131072.0);
    (h, h[0] * h[0] + h[1] * h[1] + h[2] * h[2] > 1.0)
}
/// The left gun's muzzle forward, normalised; None when too short or the muzzle sits at the origin.
pub fn muzzle_forward(position: [f32; 3], forward: [f32; 3]) -> Option<[f32; 3]> {
    let fl2 = forward[0] * forward[0] + forward[1] * forward[1] + forward[2] * forward[2];
    if !(fl2 > 0.25) || !(position[0] * position[0] + position[1] * position[1] + position[2] * position[2] > 1.0) {
        return None;
    }
    let inv = 1.0 / fl2.sqrt();
    Some([forward[0] * inv, forward[1] * inv, forward[2] * inv])
}
/// The ray from one eye to the bullet's point at `distance`, in the camera's frame (x right, y forward, z up).
#[allow(clippy::too_many_arguments)]
pub fn eye_ray(muzzle: [f32; 3], forward: [f32; 3], distance: f32, head: [f32; 3], right: [f32; 3], half_ipd: f32, sign: f32, q: [f32; 4]) -> [f32; 3] {
    let d = [0, 1, 2].map(|i| (muzzle[i] + forward[i] * distance) - (head[i] + right[i] * half_ipd * sign));
    rotate(d, [-q[0], -q[1], -q[2], q[3]])
}
/// MAIN's eye sign.
pub fn main_sign(engine_stereo: bool, main_is_right: bool, eye_sign: i32) -> f32 {
    if engine_stereo {
        if main_is_right { 1.0 } else { -1.0 }
    } else if eye_sign >= 0 {
        1.0
    } else {
        -1.0
    }
}
/// The second eye's parallax at the sight's zero distance, in NDC (0 without a zero, a projection or an IPD).
pub fn parallax(zero_m: f32, tans: Option<f32>, ipd: f32, main_is_right: bool) -> f32 {
    let mut dx = 0.0f32;
    if zero_m > 0.1 {
        if let Some(thx) = tans {
            if ipd > 0.001 {
                dx = -(ipd / zero_m) / thx;
                if main_is_right {
                    dx = -dx;
                }
            }
        }
    }
    dx
}
/// A pixel's NDC.
pub fn ndc(pixel: [f32; 2], size: [f32; 2]) -> [f32; 2] {
    [(pixel[0] / size[0]) * 2.0 - 1.0, 1.0 - (pixel[1] / size[1]) * 2.0]
}
/// An NDC point's pixel in `eye`'s half of the backbuffer.
pub fn eye_pixel(point: [f32; 2], eye: i32, size: [f32; 2]) -> [f32; 2] {
    let left = eye as f32 * size[0];
    [left + (point[0] * 0.5 + 0.5) * size[0], (0.5 - point[1] * 0.5) * size[1]]
}

// ---- the C ABI

/// # Safety
/// `q` holds four floats, `out` three.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_bd_camera_right(q: *const [f32; 4], out: *mut [f32; 3]) {
    unsafe { *out = camera_right(*q) };
}
#[no_mangle]
pub extern "C-unwind" fn cvr_bd_half_ipd(published: f32, slot: f32) -> f32 {
    half_ipd(published, slot)
}
/// # Safety
/// `fixed` holds three values, `out` three floats.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_bd_head_center(fixed: *const [i32; 3], out: *mut [f32; 3]) -> bool {
    let (h, real) = head_center(unsafe { *fixed });
    unsafe { *out = h };
    real
}
/// # Safety
/// `position` holds three floats; `forward` three, normalised in place when admitted.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_bd_muzzle_forward(position: *const [f32; 3], forward: *mut [f32; 3]) -> bool {
    match muzzle_forward(unsafe { *position }, unsafe { *forward }) {
        Some(f) => {
            unsafe { *forward = f };
            true
        }
        None => false,
    }
}
/// # Safety
/// The vectors hold their floats; `out` three.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_bd_eye_ray(
    muzzle: *const [f32; 3],
    forward: *const [f32; 3],
    distance: f32,
    head: *const [f32; 3],
    right: *const [f32; 3],
    half_ipd: f32,
    sign: f32,
    q: *const [f32; 4],
    out: *mut [f32; 3],
) {
    unsafe { *out = eye_ray(*muzzle, *forward, distance, *head, *right, half_ipd, sign, *q) };
}
#[no_mangle]
pub extern "C-unwind" fn cvr_bd_main_sign(engine_stereo: bool, main_is_right: bool, eye_sign: i32) -> f32 {
    main_sign(engine_stereo, main_is_right, eye_sign)
}
#[no_mangle]
pub extern "C-unwind" fn cvr_bd_parallax(zero_m: f32, have_tans: bool, thx: f32, ipd: f32, main_is_right: bool) -> f32 {
    parallax(zero_m, have_tans.then_some(thx), ipd, main_is_right)
}
/// # Safety
/// `out` holds two floats.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_bd_ndc(x: f32, y: f32, width: f32, height: f32, out: *mut [f32; 2]) {
    unsafe { *out = ndc([x, y], [width, height]) };
}
/// # Safety
/// `out` holds two floats.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_bd_eye_pixel(x: f32, y: f32, eye: i32, width: f32, height: f32, out: *mut [f32; 2]) {
    unsafe { *out = eye_pixel([x, y], eye, [width, height]) };
}

#[cfg(test)]
mod tests {
    //! Each.
    use super::*;

    #[test]
    fn axes_and_ipd() {
        assert_eq!(camera_right([0.0, 0.0, 0.0, 1.0]), [1.0, 0.0, 0.0]);
        // A quarter turn about z turns the right axis to +y.
        let s = core::f32::consts::FRAC_1_SQRT_2;
        let r = camera_right([0.0, 0.0, s, s]);
        assert!(r[0].abs() < 1e-6 && (r[1] - 1.0).abs() < 1e-6 && r[2].abs() < 1e-6, "{r:?}");
        assert_eq!(half_ipd(0.031, 0.04), 0.031);
        assert_eq!(half_ipd(0.0, 0.04), 0.04);
        assert_eq!(half_ipd(f32::NAN, 0.0), 0.0325);
        assert_eq!(half_ipd(0.0001, 0.0001), 0.0325);
        // Just above the threshold is a real half IPD, from either source.
        assert_eq!(half_ipd(0.0, 0.0005), 0.0005);
        assert_eq!(half_ipd(0.0005, 0.0), 0.0005);
    }
    #[test]
    fn centres_and_muzzles() {
        assert_eq!(head_center([131072, -262144, 65536]), ([1.0, -2.0, 0.5], true));
        assert!(!head_center([65536, 65536, 65536]).1);
        assert_eq!(muzzle_forward([2.0, 0.0, 0.0], [0.0, 3.0, 4.0]), Some([0.0, 0.6, 0.8]));
        assert_eq!(muzzle_forward([2.0, 0.0, 0.0], [0.0, 0.5, 0.0]), None);
        assert_eq!(muzzle_forward([0.5, 0.5, 0.0], [0.0, 1.0, 0.0]), None);
        assert_eq!(muzzle_forward([2.0, 0.0, 0.0], [f32::NAN, 1.0, 0.0]), None);
    }
    #[test]
    fn rays() {
        // Identity camera at the origin, MAIN on the right: the bullet 10 m down +y from (0.1, 0, 0).
        let r = eye_ray([0.1, 0.0, 0.0], [0.0, 1.0, 0.0], 10.0, [0.0; 3], [1.0, 0.0, 0.0], 0.03, 1.0, [0.0, 0.0, 0.0, 1.0]);
        assert!((r[0] - 0.07).abs() < 1e-6 && (r[1] - 10.0).abs() < 1e-6 && r[2].abs() < 1e-6, "{r:?}");
        let l = eye_ray([0.1, 0.0, 0.0], [0.0, 1.0, 0.0], 10.0, [0.0; 3], [1.0, 0.0, 0.0], 0.03, -1.0, [0.0, 0.0, 0.0, 1.0]);
        assert!((l[0] - 0.13).abs() < 1e-6, "{l:?}");
        // A camera turned a quarter about z sees +y world as +x... rotate's inverse brings it back.
        let s = core::f32::consts::FRAC_1_SQRT_2;
        let t = eye_ray([0.0; 3], [0.0, 1.0, 0.0], 1.0, [0.0; 3], [0.0, 0.0, 0.0], 0.0, 1.0, [0.0, 0.0, s, s]);
        assert!((t[0] - 1.0).abs() < 1e-6 && t[1].abs() < 1e-6, "{t:?}");
    }
    #[test]
    fn signs_and_parallax() {
        assert_eq!(main_sign(true, true, -1), 1.0);
        assert_eq!(main_sign(true, false, 1), -1.0);
        assert_eq!(main_sign(false, true, 0), 1.0);
        assert_eq!(main_sign(false, true, -1), -1.0);
        assert_eq!(parallax(20.0, Some(0.5), 0.064, false), -(0.064 / 20.0) / 0.5);
        assert_eq!(parallax(20.0, Some(0.5), 0.064, true), (0.064 / 20.0) / 0.5);
        assert_eq!(parallax(0.1, Some(0.5), 0.064, false), 0.0);
        assert_eq!(parallax(20.0, None, 0.064, false), 0.0);
        assert_eq!(parallax(20.0, Some(0.5), 0.001, false), 0.0);
    }
    #[test]
    fn pixels() {
        assert_eq!(ndc([50.0, 25.0], [100.0, 50.0]), [0.0, 0.0]);
        assert_eq!(ndc([100.0, 0.0], [100.0, 50.0]), [1.0, 1.0]);
        assert_eq!(eye_pixel([0.0, 0.0], 0, [100.0, 50.0]), [50.0, 25.0]);
        assert_eq!(eye_pixel([1.0, -1.0], 1, [100.0, 50.0]), [200.0, 50.0]);
        let p = [37.0, 11.0];
        let back = eye_pixel(ndc(p, [100.0, 50.0]), 0, [100.0, 50.0]);
        assert!((back[0] - p[0]).abs() < 1e-4 && (back[1] - p[1]).abs() < 1e-4);
    }
}
