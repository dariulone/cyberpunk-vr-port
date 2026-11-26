//! DLSS frame generation's camera matrices (src/Framegen/DlssFrameGen.cpp, `BuildMatrices` and its helpers): the
//! current camera's view-to-clip projection (from the vertical field of view, the aspect, the near and far planes and
//! the off-centre shift; reversed depth when inverted, an infinite far plane when asked or not finite) and its inverse;
//! and, given the previous camera, the clip-to-previous-clip chain -- this frame's clip to view, view to world (both
//! views placed relative to this camera's position), world to the previous view, the previous projection -- and its
//! inverse. Matrices are row-major float[16], composed left to right (`a * b` applies `a` first), as the .cpp's are.
//!
//! In the Rust build `BuildMatrices` forwards to `cvr_dlssg_build_matrices`; the unit tests below check the
//! projections, the inverse and the chain.
use crate::frame_capture::CameraData;

pub type Mat = [f32; 16];

/// Matrices (include/Framegen/DlssFrameGen.hpp).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Matrices {
    pub view_to_clip: Mat,
    pub clip_to_view: Mat,
    pub clip_to_prev_clip: Mat,
    pub prev_clip_to_clip: Mat,
}
const _: () = assert!(core::mem::size_of::<Matrices>() == 256);

pub fn multiply(a: &Mat, b: &Mat) -> Mat {
    let mut r = [0.0f32; 16];
    for i in 0..4 {
        for j in 0..4 {
            let mut s = 0.0f32;
            for k in 0..4 {
                s += a[i * 4 + k] * b[k * 4 + j];
            }
            r[i * 4 + j] = s;
        }
    }
    r
}
/// The inverse by cofactors, none when the determinant is not finite or below 1e-20 in magnitude.
#[rustfmt::skip]
pub fn invert(m: &Mat) -> Option<Mat> {
    let mut inv = [0.0f32; 16];
    inv[0]=m[5]*m[10]*m[15]-m[5]*m[11]*m[14]-m[9]*m[6]*m[15]+m[9]*m[7]*m[14]+m[13]*m[6]*m[11]-m[13]*m[7]*m[10];
    inv[4]=-m[4]*m[10]*m[15]+m[4]*m[11]*m[14]+m[8]*m[6]*m[15]-m[8]*m[7]*m[14]-m[12]*m[6]*m[11]+m[12]*m[7]*m[10];
    inv[8]=m[4]*m[9]*m[15]-m[4]*m[11]*m[13]-m[8]*m[5]*m[15]+m[8]*m[7]*m[13]+m[12]*m[5]*m[11]-m[12]*m[7]*m[9];
    inv[12]=-m[4]*m[9]*m[14]+m[4]*m[10]*m[13]+m[8]*m[5]*m[14]-m[8]*m[6]*m[13]-m[12]*m[5]*m[10]+m[12]*m[6]*m[9];
    inv[1]=-m[1]*m[10]*m[15]+m[1]*m[11]*m[14]+m[9]*m[2]*m[15]-m[9]*m[3]*m[14]-m[13]*m[2]*m[11]+m[13]*m[3]*m[10];
    inv[5]=m[0]*m[10]*m[15]-m[0]*m[11]*m[14]-m[8]*m[2]*m[15]+m[8]*m[3]*m[14]+m[12]*m[2]*m[11]-m[12]*m[3]*m[10];
    inv[9]=-m[0]*m[9]*m[15]+m[0]*m[11]*m[13]+m[8]*m[1]*m[15]-m[8]*m[3]*m[13]-m[12]*m[1]*m[11]+m[12]*m[3]*m[9];
    inv[13]=m[0]*m[9]*m[14]-m[0]*m[10]*m[13]-m[8]*m[1]*m[14]+m[8]*m[2]*m[13]+m[12]*m[1]*m[10]-m[12]*m[2]*m[9];
    inv[2]=m[1]*m[6]*m[15]-m[1]*m[7]*m[14]-m[5]*m[2]*m[15]+m[5]*m[3]*m[14]+m[13]*m[2]*m[7]-m[13]*m[3]*m[6];
    inv[6]=-m[0]*m[6]*m[15]+m[0]*m[7]*m[14]+m[4]*m[2]*m[15]-m[4]*m[3]*m[14]-m[12]*m[2]*m[7]+m[12]*m[3]*m[6];
    inv[10]=m[0]*m[5]*m[15]-m[0]*m[7]*m[13]-m[4]*m[1]*m[15]+m[4]*m[3]*m[13]+m[12]*m[1]*m[7]-m[12]*m[3]*m[5];
    inv[14]=-m[0]*m[5]*m[14]+m[0]*m[6]*m[13]+m[4]*m[1]*m[14]-m[4]*m[2]*m[13]-m[12]*m[1]*m[6]+m[12]*m[2]*m[5];
    inv[3]=-m[1]*m[6]*m[11]+m[1]*m[7]*m[10]+m[5]*m[2]*m[11]-m[5]*m[3]*m[10]-m[9]*m[2]*m[7]+m[9]*m[3]*m[6];
    inv[7]=m[0]*m[6]*m[11]-m[0]*m[7]*m[10]-m[4]*m[2]*m[11]+m[4]*m[3]*m[10]+m[8]*m[2]*m[7]-m[8]*m[3]*m[6];
    inv[11]=-m[0]*m[5]*m[11]+m[0]*m[7]*m[9]+m[4]*m[1]*m[11]-m[4]*m[3]*m[9]-m[8]*m[1]*m[7]+m[8]*m[3]*m[5];
    inv[15]=m[0]*m[5]*m[10]-m[0]*m[6]*m[9]-m[4]*m[1]*m[10]+m[4]*m[2]*m[9]+m[8]*m[1]*m[6]-m[8]*m[2]*m[5];
    let det = m[0]*inv[0]+m[1]*inv[4]+m[2]*inv[8]+m[3]*inv[12];
    if !det.is_finite() || det.abs() < 1e-20 {
        return None;
    }
    Some(inv.map(|v| v / det))
}
pub fn identity() -> Mat {
    let mut m = [0.0f32; 16];
    (m[0], m[5], m[10], m[15]) = (1.0, 1.0, 1.0, 1.0);
    m
}
/// The camera's world-to-view, its position taken relative to `origin`.
pub fn view(c: &CameraData, origin: &[f32; 3]) -> Mat {
    let p = [c.position[0] - origin[0], c.position[1] - origin[1], c.position[2] - origin[2]];
    let (r, u, f) = (&c.right, &c.up, &c.forward);
    [
        r[0], u[0], f[0], 0.0, r[1], u[1], f[1], 0.0, r[2], u[2], f[2], 0.0,
        -(p[0] * r[0] + p[1] * r[1] + p[2] * r[2]),
        -(p[0] * u[0] + p[1] * u[1] + p[2] * u[2]),
        -(p[0] * f[0] + p[1] * f[1] + p[2] * f[2]),
        1.0,
    ]
}
/// The camera's view-to-clip.
pub fn projection(c: &CameraData) -> Mat {
    let ys = 1.0f32 / (c.vertical_fov * 0.5).tan();
    let (xs, n, f) = (ys / c.aspect, c.near_plane, c.far_plane);
    let (a, b) = if c.inverted {
        if c.infinite || !f.is_finite() { (0.0, n) } else { (-n / (f - n), n * f / (f - n)) }
    } else if c.infinite || !f.is_finite() {
        (1.0, -n)
    } else {
        (f / (f - n), -n * f / (f - n))
    };
    [xs, 0.0, 0.0, 0.0, 0.0, ys, 0.0, 0.0, c.shift[0], c.shift[1], a, 1.0, 0.0, 0.0, b, 0.0]
}
/// BuildMatrices: false when the camera is unusable or a matrix does not invert (`out` then partly written, as the
/// .cpp leaves it).
pub fn build(current: &CameraData, previous: Option<&CameraData>, out: &mut Matrices) -> bool {
    if !(current.aspect > 0.0) || !(current.vertical_fov > 0.0) || !(current.near_plane > 0.0) {
        return false;
    }
    out.view_to_clip = projection(current);
    let Some(clip_to_view) = invert(&out.view_to_clip) else { return false };
    out.clip_to_view = clip_to_view;
    let Some(previous) = previous else {
        out.clip_to_prev_clip = identity();
        out.prev_clip_to_clip = identity();
        return true;
    };
    let view_now = view(current, &current.position);
    let view_then = view(previous, &current.position);
    let projection_then = projection(previous);
    let Some(view_to_world) = invert(&view_now) else { return false };
    let chain = multiply(&multiply(&out.clip_to_view, &view_to_world), &view_then);
    out.clip_to_prev_clip = multiply(&chain, &projection_then);
    match invert(&out.clip_to_prev_clip) {
        Some(m) => {
            out.prev_clip_to_clip = m;
            true
        }
        None => false,
    }
}

// ---- the C ABI

/// # Safety
/// `current` is valid, `previous` null or valid, `out` null or writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_dlssg_build_matrices(current: *const CameraData, previous: *const CameraData, out: *mut Matrices) -> bool {
    if out.is_null() || current.is_null() {
        return false;
    }
    // SAFETY: as the caller vouches.
    unsafe { build(&*current, previous.as_ref(), &mut *out) }
}

#[cfg(test)]
mod tests {
    //! Projections, the inverse and the chain.
    use super::*;

    fn camera() -> CameraData {
        CameraData {
            position: [1.0, 2.0, 3.0],
            right: [1.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            forward: [0.0, 0.0, 1.0],
            near_plane: 0.1,
            far_plane: 100.0,
            vertical_fov: core::f32::consts::FRAC_PI_2,
            aspect: 2.0,
            ..Default::default()
        }
    }
    fn near(a: &Mat, b: &Mat) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4)
    }
    #[test]
    fn projections() {
        let mut c = camera();
        let p = projection(&c);
        // A 90 degree field: ys 1, xs 1/2; standard depth f/(f-n) and -nf/(f-n).
        assert!((p[5] - 1.0).abs() < 1e-6 && (p[0] - 0.5).abs() < 1e-6);
        assert!((p[10] - 100.0 / 99.9).abs() < 1e-5 && (p[14] + 10.0 / 99.9).abs() < 1e-5 && p[11] == 1.0);
        c.inverted = true;
        let p = projection(&c);
        assert!((p[10] + 0.1 / 99.9).abs() < 1e-6 && (p[14] - 10.0 / 99.9).abs() < 1e-5);
        c.infinite = true;
        assert_eq!((projection(&c)[10], projection(&c)[14]), (0.0, 0.1));
        c.inverted = false;
        assert_eq!((projection(&c)[10], projection(&c)[14]), (1.0, -0.1));
        c.infinite = false;
        c.far_plane = f32::INFINITY;
        assert_eq!(projection(&c)[10], 1.0);
        c.shift = [0.25, -0.5];
        assert_eq!((projection(&c)[8], projection(&c)[9]), (0.25, -0.5));
    }
    #[test]
    fn inverses() {
        let p = projection(&camera());
        let inv = invert(&p).unwrap();
        assert!(near(&multiply(&p, &inv), &identity()));
        assert!(invert(&[0.0; 16]).is_none());
        assert_eq!(invert(&identity()), Some(identity()));
    }
    #[test]
    fn chains() {
        let c = camera();
        let mut m = Matrices { view_to_clip: [0.0; 16], clip_to_view: [0.0; 16], clip_to_prev_clip: [0.0; 16], prev_clip_to_clip: [0.0; 16] };
        // No previous camera: identities.
        assert!(build(&c, None, &mut m));
        assert_eq!((m.clip_to_prev_clip, m.prev_clip_to_clip), (identity(), identity()));
        // The same camera again: the chain is the identity.
        assert!(build(&c, Some(&c), &mut m));
        assert!(near(&m.clip_to_prev_clip, &identity()) && near(&m.prev_clip_to_clip, &identity()));
        // A camera that moved: the chain carries this view's points to where the previous view saw them.
        let mut then = c;
        then.position[0] -= 1.0;
        assert!(build(&c, Some(&then), &mut m));
        assert!(!near(&m.clip_to_prev_clip, &identity()));
        assert!(near(&multiply(&m.clip_to_prev_clip, &m.prev_clip_to_clip), &identity()));
        // An unusable camera.
        let mut bad = c;
        bad.aspect = 0.0;
        assert!(!build(&bad, None, &mut m));
    }
}
