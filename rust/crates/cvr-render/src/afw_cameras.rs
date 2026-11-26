//! AFW's two cameras for PureDark's plugin (formerly part of src/Render/Afw.cpp): column-major 4x4 (m[col * 4 + row]);
//! view space right-handed (x right, y up, -z forward) -- the handedness does not matter to the plugin as long as the
//! matrices agree (tools/afw_tests/afw_replay.cpp); the projection's depth is the engine's own convention (reversed:
//! near 1, far 0), with the depth texture as it is. Computed in doubles as the C++ did, the tangents through the C
//! runtime's tanf as <cmath>'s std::tan(float) calls it.
use cvr_gpu::frame_capture::CameraData;

extern "C" {
    fn tanf(x: f32) -> f32;
}

/// pd::Mat4.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct Mat4 {
    pub(crate) m: [f32; 16],
}

#[derive(Clone, Copy, Default)]
pub(crate) struct M4 {
    pub(crate) m: [f64; 16],
}
impl M4 {
    pub(crate) fn set(&mut self, r: usize, c: usize, v: f64) {
        self.m[c * 4 + r] = v;
    }
}

#[rustfmt::skip]
pub(crate) fn inverse(a: &M4) -> M4 {
    let m = &a.m;
    let mut inv = [0f64; 16];
    inv[0] = m[5] * m[10] * m[15] - m[5] * m[11] * m[14] - m[9] * m[6] * m[15] + m[9] * m[7] * m[14] + m[13] * m[6] * m[11] - m[13] * m[7] * m[10];
    inv[4] = -m[4] * m[10] * m[15] + m[4] * m[11] * m[14] + m[8] * m[6] * m[15] - m[8] * m[7] * m[14] - m[12] * m[6] * m[11] + m[12] * m[7] * m[10];
    inv[8] = m[4] * m[9] * m[15] - m[4] * m[11] * m[13] - m[8] * m[5] * m[15] + m[8] * m[7] * m[13] + m[12] * m[5] * m[11] - m[12] * m[7] * m[9];
    inv[12] = -m[4] * m[9] * m[14] + m[4] * m[10] * m[13] + m[8] * m[5] * m[14] - m[8] * m[6] * m[13] - m[12] * m[5] * m[10] + m[12] * m[6] * m[9];
    inv[1] = -m[1] * m[10] * m[15] + m[1] * m[11] * m[14] + m[9] * m[2] * m[15] - m[9] * m[3] * m[14] - m[13] * m[2] * m[11] + m[13] * m[3] * m[10];
    inv[5] = m[0] * m[10] * m[15] - m[0] * m[11] * m[14] - m[8] * m[2] * m[15] + m[8] * m[3] * m[14] + m[12] * m[2] * m[11] - m[12] * m[3] * m[10];
    inv[9] = -m[0] * m[9] * m[15] + m[0] * m[11] * m[13] + m[8] * m[1] * m[15] - m[8] * m[3] * m[13] - m[12] * m[1] * m[11] + m[12] * m[3] * m[9];
    inv[13] = m[0] * m[9] * m[14] - m[0] * m[10] * m[13] - m[8] * m[1] * m[14] + m[8] * m[2] * m[13] + m[12] * m[1] * m[10] - m[12] * m[2] * m[9];
    inv[2] = m[1] * m[6] * m[15] - m[1] * m[7] * m[14] - m[5] * m[2] * m[15] + m[5] * m[3] * m[14] + m[13] * m[2] * m[7] - m[13] * m[3] * m[6];
    inv[6] = -m[0] * m[6] * m[15] + m[0] * m[7] * m[14] + m[4] * m[2] * m[15] - m[4] * m[3] * m[14] - m[12] * m[2] * m[7] + m[12] * m[3] * m[6];
    inv[10] = m[0] * m[5] * m[15] - m[0] * m[7] * m[13] - m[4] * m[1] * m[15] + m[4] * m[3] * m[13] + m[12] * m[1] * m[7] - m[12] * m[3] * m[5];
    inv[14] = -m[0] * m[5] * m[14] + m[0] * m[6] * m[13] + m[4] * m[1] * m[14] - m[4] * m[2] * m[13] - m[12] * m[1] * m[6] + m[12] * m[2] * m[5];
    inv[3] = -m[1] * m[6] * m[11] + m[1] * m[7] * m[10] + m[5] * m[2] * m[11] - m[5] * m[3] * m[10] - m[9] * m[2] * m[7] + m[9] * m[3] * m[6];
    inv[7] = m[0] * m[6] * m[11] - m[0] * m[7] * m[10] - m[4] * m[2] * m[11] + m[4] * m[3] * m[10] + m[8] * m[2] * m[7] - m[8] * m[3] * m[6];
    inv[11] = -m[0] * m[5] * m[11] + m[0] * m[7] * m[9] + m[4] * m[1] * m[11] - m[4] * m[3] * m[9] - m[8] * m[1] * m[7] + m[8] * m[3] * m[5];
    inv[15] = m[0] * m[5] * m[10] - m[0] * m[6] * m[9] - m[4] * m[1] * m[10] + m[4] * m[2] * m[9] + m[8] * m[1] * m[6] - m[8] * m[2] * m[5];
    let mut det = m[0] * inv[0] + m[1] * inv[4] + m[2] * inv[8] + m[3] * inv[12];
    let mut o = M4::default();
    if !(det.abs() > 1e-300) {
        return o;
    }
    det = 1.0 / det;
    for i in 0..16 {
        o.m[i] = inv[i] * det;
    }
    o
}

pub(crate) fn to_mat(a: &M4) -> Mat4 {
    let mut o = Mat4::default();
    for i in 0..16 {
        o.m[i] = a.m[i] as f32;
    }
    o
}

pub(crate) fn world_to_view(position: &[f32; 3], c: &CameraData) -> M4 {
    let ax: [[f64; 3]; 3] = [
        [
            f64::from(c.right[0]),
            f64::from(c.right[1]),
            f64::from(c.right[2]),
        ],
        [f64::from(c.up[0]), f64::from(c.up[1]), f64::from(c.up[2])],
        [
            f64::from(-c.forward[0]),
            f64::from(-c.forward[1]),
            f64::from(-c.forward[2]),
        ],
    ];
    let mut v = M4::default();
    for r in 0..3 {
        let mut t = 0.0f64;
        for k in 0..3 {
            v.set(r, k, ax[r][k]);
            t += ax[r][k] * f64::from(position[k]);
        }
        v.set(r, 3, -t);
    }
    v.set(3, 3, 1.0);
    v
}

/// XrFovf: left, right, up, down.
pub(crate) fn view_to_clip(fov: &[f32; 4], c: &CameraData) -> M4 {
    // SAFETY: the C runtime's tanf.
    let (l, r, u, d) = unsafe {
        (
            f64::from(tanf(fov[0])),
            f64::from(tanf(fov[1])),
            f64::from(tanf(fov[2])),
            f64::from(tanf(fov[3])),
        )
    };
    let n = if c.near_plane > 0.0 {
        f64::from(c.near_plane)
    } else {
        0.02
    };
    let f = if f64::from(c.far_plane) > n {
        f64::from(c.far_plane)
    } else {
        16000.0
    };
    // depth = A + B / z for the forward distance z
    let (a, b) = if c.infinite {
        if c.inverted {
            (0.0, n)
        } else {
            (1.0, -n)
        }
    } else if c.inverted {
        (-n / (f - n), n * f / (f - n))
    } else {
        (f / (f - n), -n * f / (f - n))
    };
    let mut p = M4::default(); // view z is backward: forward distance = -z
    p.set(0, 0, 2.0 / (r - l));
    p.set(0, 2, (r + l) / (r - l));
    p.set(1, 1, 2.0 / (u - d));
    p.set(1, 2, (u + d) / (u - d));
    p.set(2, 2, -a);
    p.set(2, 3, b);
    p.set(3, 2, -1.0);
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inverse_of_a_view() {
        let mut c = CameraData::default();
        c.right = [1.0, 0.0, 0.0];
        c.up = [0.0, 0.0, 1.0];
        c.forward = [0.0, 1.0, 0.0];
        let v = world_to_view(&[1.0, 2.0, 3.0], &c);
        let i = inverse(&v);
        // The view's inverse puts the eye back at its position.
        assert_eq!((i.m[12], i.m[13], i.m[14], i.m[15]), (1.0, 2.0, 3.0, 1.0));
    }

    #[test]
    fn reversed_depth_projection() {
        let mut c = CameraData::default();
        c.near_plane = 0.1;
        c.far_plane = 100.0;
        c.inverted = true;
        let p = view_to_clip(&[-0.5, 0.5, 0.5, -0.5], &c);
        // depth = A + B / z: at the near plane 1, at the far 0.
        let a = -p.m[2 * 4 + 2];
        let b = p.m[3 * 4 + 2];
        let n = f64::from(0.1f32);
        let f = f64::from(100.0f32);
        assert!((a + b / n - 1.0).abs() < 1e-12 && (a + b / f).abs() < 1e-12);
    }

    /// The same 20000 pseudo-random cameras as the C++ reference (the reference branch's functions copied verbatim
    /// into a program built like the DLL, /O2 /fp:precise): every output float's bits hashed (FNV-1a) the same.
    #[test]
    fn parity_with_the_cpp_cameras() {
        let mut s: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut step = || {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            s
        };
        let mut next = |lo: f32, hi: f32, step: &mut dyn FnMut() -> u64| {
            let u = (step() >> 11) as f64 / 9007199254740992.0;
            (f64::from(lo) + f64::from(hi - lo) * u) as f32
        };
        let mut h: u64 = 1469598103934665603;
        let mut mix = |m: &Mat4| {
            for f in m.m {
                let b = f.to_bits();
                for i in 0..4 {
                    h ^= u64::from((b >> (8 * i)) & 0xFF);
                    h = h.wrapping_mul(1099511628211);
                }
            }
        };
        for _ in 0..20000 {
            let mut c = CameraData::default();
            for k in 0..3 {
                c.position[k] = next(-100.0, 100.0, &mut step);
            }
            for k in 0..3 {
                c.right[k] = next(-1.0, 1.0, &mut step);
            }
            for k in 0..3 {
                c.up[k] = next(-1.0, 1.0, &mut step);
            }
            for k in 0..3 {
                c.forward[k] = next(-1.0, 1.0, &mut step);
            }
            c.near_plane = next(-0.1, 0.5, &mut step);
            c.far_plane = next(-1.0, 20000.0, &mut step);
            let bits = step();
            c.inverted = bits & 1 != 0;
            c.infinite = bits & 6 == 6;
            let fov = [
                next(-1.2, -0.3, &mut step),
                next(0.3, 1.2, &mut step),
                next(0.3, 1.2, &mut step),
                next(-1.2, -0.3, &mut step),
            ];
            let mut other = [0f32; 3];
            for o in &mut other {
                *o = next(-100.0, 100.0, &mut step);
            }
            let sv = world_to_view(&c.position, &c);
            let dv = world_to_view(&other, &c);
            let sp = view_to_clip(&fov, &c);
            mix(&to_mat(&sv));
            mix(&to_mat(&inverse(&sv)));
            mix(&to_mat(&dv));
            mix(&to_mat(&inverse(&dv)));
            mix(&to_mat(&sp));
            mix(&to_mat(&inverse(&sp)));
        }
        assert_eq!(format!("{h:016x}"), "c57c186238919cde");
    }
}
