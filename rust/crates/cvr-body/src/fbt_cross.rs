//! The FBT calibration's step 3 cross (src/Fbt/Game/Calibration.cpp, `SetUpCross`, `CrossTick`, `Mark`,
//! `HeadPitchDeg` and `ArmElevationDeg`): the cross's frame -- centred on the feet trackers' midpoint and square to the
//! line between them (turned to face the way step 1's level gaze looked; the gaze's heading when the feet are within
//! 10 cm; from step 1's head without both feet; refused when the gaze was within 0.3 of vertical) -- and its size at
//! the player's scale (1.8 m ahead, the top at the eyes, the arms at the shoulders, the half span V's half shoulders and
//! arm); each frame's marks (the head on the top: sideways, height and a level gaze; a hand on its arm: height, depth
//! and out along it as a share of the half span) and their dots laid on the cross's plane; a mark's grade against its
//! tolerance (2 on it, 1 within twice, 0 off); the view's pitch; and an arm's elevation.
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_fbt_*` (the logging, the hold countdown and the
//! calibrator's measurements stay the .cpp's); the unit tests below cover each rule. The float arithmetic follows the
//! .cpp's order.

pub type V3 = [f32; 3];
pub type Q4 = [f32; 4];

pub const CROSS_AHEAD: f32 = 1.8;
pub const HEAD_SIDE: f32 = 0.10;
pub const HEAD_HEIGHT: f32 = 0.07;
pub const HAND_HEIGHT: f32 = 0.08;
pub const HAND_OUT: f32 = 0.75;
pub const HAND_DEPTH: f32 = 0.30;
pub const LEVEL_DEG: f32 = 8.0;

fn mul(a: Q4, b: Q4) -> Q4 {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}
/// The .cpp's Rotate: q v q*.
pub fn rotate(q: Q4, v: V3) -> V3 {
    let r = mul(mul(q, [v[0], v[1], v[2], 0.0]), [-q[0], -q[1], -q[2], q[3]]);
    [r[0], r[1], r[2]]
}
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
/// Mark: 2 on it, 1 near (twice the tolerance), 0 off.
pub fn mark(error: f32, tolerance: f32) -> i32 {
    if error <= tolerance {
        2
    } else if error <= 2.0 * tolerance {
        1
    } else {
        0
    }
}
/// HeadPitchDeg: the view's forward's elevation.
pub fn head_pitch_deg(q: Q4) -> f32 {
    let f = rotate(q, [0.0, 0.0, -1.0]);
    let y = if f[1] > 1.0 { 1.0 } else if f[1] < -1.0 { -1.0 } else { f[1] };
    y.asin() * 57.29578
}
/// ArmElevationDeg (model space, z up).
pub fn arm_elevation_deg(upper: V3, fore: V3) -> f32 {
    let (dx, dy, dz) = (fore[0] - upper[0], fore[1] - upper[1], fore[2] - upper[2]);
    dz.atan2((dx * dx + dy * dy).sqrt()) * 57.29578
}
/// The cross's frame and where it came from.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Frame {
    pub spot: V3,
    pub fwd: V3,
    pub right: V3,
    /// 0 the head (no feet trackers), 1 the feet with the gaze's heading (too close for their line), 2 the feet.
    pub from: u32,
    /// Step 1's head sideways of the feet's midpoint (with feet).
    pub off: f32,
}
/// SetUpCross's frame from step 1's level head and the feet (None: the gaze too steep).
pub fn cross_frame(head_pos: V3, head_rot: Q4, feet: [Option<V3>; 2]) -> Option<Frame> {
    let f3 = rotate(head_rot, [0.0, 0.0, -1.0]);
    let fl = (f3[0] * f3[0] + f3[2] * f3[2]).sqrt();
    if fl < 0.3 {
        return None;
    }
    let mut x = Frame::default();
    x.fwd = [f3[0] / fl, 0.0, f3[2] / fl];
    x.right = [-x.fwd[2], 0.0, x.fwd[0]];
    x.spot = [head_pos[0], 0.0, head_pos[2]];
    if let [Some(a), Some(b)] = feet {
        let (mut rx, mut rz) = (b[0] - a[0], b[2] - a[2]);
        let rl = (rx * rx + rz * rz).sqrt();
        x.spot = [0.5 * (a[0] + b[0]), 0.0, 0.5 * (a[2] + b[2])];
        x.from = 1;
        if rl > 0.10 {
            rx /= rl;
            rz /= rl;
            if rx * x.right[0] + rz * x.right[2] < 0.0 {
                rx = -rx;
                rz = -rz;
            }
            x.right = [rx, 0.0, rz];
            x.fwd = [rz, 0.0, -rx];
            x.from = 2;
        }
        x.off = (head_pos[0] - x.spot[0]) * x.right[0] + (head_pos[2] - x.spot[2]) * x.right[2];
    }
    Some(x)
}
/// The cross's size: (base, top, arms' height, half span).
pub fn cross_dims(frame: &Frame, player_eye: f32, avatar_eye: f32, shoulders_z: [f32; 2], shoulders_x: [f32; 2], arms: [f32; 2]) -> (V3, f32, f32, f32) {
    let k = player_eye / avatar_eye;
    let base = [frame.spot[0] + frame.fwd[0] * CROSS_AHEAD, 0.0, frame.spot[2] + frame.fwd[2] * CROSS_AHEAD];
    let shoulder_z = 0.5 * (shoulders_z[0] + shoulders_z[1]);
    let bar = player_eye - (avatar_eye - shoulder_z) * k;
    let half_shoulders = 0.5 * (shoulders_x[1] - shoulders_x[0]).abs();
    let half_span = (half_shoulders + 0.5 * (arms[0] + arms[1])) * k;
    (base, player_eye, bar, half_span)
}
/// The cross as CrossTick reads it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Cross {
    pub spot: V3,
    pub fwd: V3,
    pub right: V3,
    pub base: V3,
    pub tip_y: f32,
    pub bar_y: f32,
    pub half_span: f32,
}
const _: () = assert!(core::mem::size_of::<Cross>() == 60 && core::mem::size_of::<Frame>() == 44);
fn on_cross(x: &Cross, p: V3) -> V3 {
    let lateral = dot(sub(p, x.spot), x.right);
    [x.base[0] + x.right[0] * lateral, p[1], x.base[2] + x.right[2] * lateral]
}
/// CrossTick's marks: writes the tracked dots and states (head, left hand, right hand).
pub fn cross_marks(x: &Cross, head: Option<(V3, Q4)>, hands: [Option<V3>; 2], dots: &mut [V3; 3], states: &mut [i32; 3]) {
    if let Some((p, q)) = head {
        dots[0] = on_cross(x, p);
        let side = dot(sub(p, x.spot), x.right).abs();
        let up = (p[1] - x.tip_y).abs();
        let level = mark(head_pitch_deg(q).abs(), LEVEL_DEG);
        states[0] = mark(side, HEAD_SIDE).min(mark(up, HEAD_HEIGHT)).min(level);
    }
    for (h, hand) in hands.iter().enumerate() {
        let Some(p) = *hand else { continue };
        dots[1 + h] = on_cross(x, p);
        let d = sub(p, x.spot);
        let along = dot(d, x.right) * if h == 0 { -1.0 } else { 1.0 };
        let reach = if x.half_span > 0.1 { along / x.half_span } else { 0.0 };
        let out = if reach >= HAND_OUT {
            2
        } else if reach >= 0.5 * HAND_OUT {
            1
        } else {
            0
        };
        states[1 + h] = mark((p[1] - x.bar_y).abs(), HAND_HEIGHT).min(mark(dot(d, x.fwd).abs(), HAND_DEPTH)).min(out);
    }
}

// ---- the C ABI

#[no_mangle]
pub extern "C-unwind" fn cvr_fbt_mark(error: f32, tolerance: f32) -> i32 {
    mark(error, tolerance)
}
/// # Safety
/// `q` holds x, y, z, w.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_fbt_head_pitch_deg(q: *const Q4) -> f32 {
    head_pitch_deg(unsafe { *q })
}
/// # Safety
/// Both points hold three floats.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_fbt_arm_elevation_deg(upper: *const V3, fore: *const V3) -> f32 {
    arm_elevation_deg(unsafe { *upper }, unsafe { *fore })
}
/// # Safety
/// The points and rotations hold their floats (a foot's ignored when not valid); `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_fbt_cross_frame(
    head_pos: *const V3,
    head_rot: *const Q4,
    left_valid: bool,
    left: *const V3,
    right_valid: bool,
    right: *const V3,
    out: *mut Frame,
) -> bool {
    let feet = unsafe { [left_valid.then(|| *left), right_valid.then(|| *right)] };
    match cross_frame(unsafe { *head_pos }, unsafe { *head_rot }, feet) {
        Some(frame) => {
            unsafe { *out = frame };
            true
        }
        None => false,
    }
}
/// # Safety
/// `frame` is valid; the pairs hold two floats; `base` holds three and the rest one each.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_fbt_cross_dims(
    frame: *const Frame,
    player_eye: f32,
    avatar_eye: f32,
    shoulders_z: *const [f32; 2],
    shoulders_x: *const [f32; 2],
    arms: *const [f32; 2],
    base: *mut V3,
    tip: *mut f32,
    bar: *mut f32,
    half_span: *mut f32,
) {
    let (b, t, r, h) = unsafe { cross_dims(&*frame, player_eye, avatar_eye, *shoulders_z, *shoulders_x, *arms) };
    unsafe {
        *base = b;
        *tip = t;
        *bar = r;
        *half_span = h;
    }
}
/// # Safety
/// `cross` is valid; the head pose and hand points hold their floats when valid; `dots` holds three points, `states`
/// three ints.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_fbt_cross_marks(
    cross: *const Cross,
    head_valid: bool,
    head_pos: *const V3,
    head_rot: *const Q4,
    left_valid: bool,
    left: *const V3,
    right_valid: bool,
    right: *const V3,
    dots: *mut [V3; 3],
    states: *mut [i32; 3],
) {
    unsafe {
        let head = head_valid.then(|| (*head_pos, *head_rot));
        cross_marks(&*cross, head, [left_valid.then(|| *left), right_valid.then(|| *right)], &mut *dots, &mut *states);
    }
}

// ---- step 2's body frame (CalibrationStepTick): the IK library's own quaternion arithmetic (FbtMath.hpp), not Rotate's

fn ik_cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
/// FbtMath.hpp's Rotate: v + t w + u x t, t = 2 u x v.
fn ik_rotate(q: Q4, v: V3) -> V3 {
    let u = [q[0], q[1], q[2]];
    let c = ik_cross(u, v);
    let t = [c[0] * 2.0, c[1] * 2.0, c[2] * 2.0];
    let s = ik_cross(u, t);
    [v[0] + t[0] * q[3] + s[0], v[1] + t[1] * q[3] + s[1], v[2] + t[2] * q[3] + s[2]]
}
/// FbtMath.hpp's quaternion Normalize.
fn ik_normalize(q: Q4) -> Q4 {
    let l = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if l > 1e-9 {
        [q[0] / l, q[1] / l, q[2] / l, q[3] / l]
    } else {
        [0.0, 0.0, 0.0, 1.0]
    }
}
/// Step 2's body frame: with both feet, the head turned to look along the feet (when they are 10 cm apart and the gaze is
/// off vertical; the way the player faces) and taken over their midpoint sideways (when its right is off vertical).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FeetFrame {
    pub rot: Q4,
    pub pos: V3,
    pub turn: f32,
    pub side: f32,
    pub turned: bool,
    pub shifted: bool,
}
/// The head's frame over the feet `lf` and `rf`.
pub fn feet_frame(rot: Q4, pos: V3, lf: V3, rf: V3) -> FeetFrame {
    let mut out = FeetFrame { rot, pos, ..Default::default() };
    let (mut fx, mut fy) = (rf[0] - lf[0], rf[1] - lf[1]);
    let fl = (fx * fx + fy * fy).sqrt();
    let h3 = ik_rotate(out.rot, [0.0, 1.0, 0.0]);
    let hl = (h3[0] * h3[0] + h3[1] * h3[1]).sqrt();
    if fl > 0.10 && hl > 0.3 {
        fx /= fl;
        fy /= fl;
        let (hx, hy) = (h3[0] / hl, h3[1] / hl);
        let (mut ffx, mut ffy) = (-fy, fx);
        if ffx * hx + ffy * hy < 0.0 {
            ffx = -ffx;
            ffy = -ffy;
        }
        let turn = (hx * ffy - hy * ffx).atan2(hx * ffx + hy * ffy);
        // AxisAngle({0, 0, 1}, turn): the axis normalized to itself.
        let (s, c) = ((turn * 0.5).sin(), (turn * 0.5).cos());
        let a = [0.0 * s, 0.0 * s, 1.0 * s, c];
        let r = out.rot;
        let product = [
            a[3] * r[0] + a[0] * r[3] + a[1] * r[2] - a[2] * r[1],
            a[3] * r[1] - a[0] * r[2] + a[1] * r[3] + a[2] * r[0],
            a[3] * r[2] + a[0] * r[1] - a[1] * r[0] + a[2] * r[3],
            a[3] * r[3] - a[0] * r[0] - a[1] * r[1] - a[2] * r[2],
        ];
        out.rot = ik_normalize(product);
        out.turn = turn;
        out.turned = true;
    }
    let r3 = ik_rotate(out.rot, [1.0, 0.0, 0.0]);
    let rl = (r3[0] * r3[0] + r3[1] * r3[1]).sqrt();
    if rl > 0.3 {
        let (rx, ry) = (r3[0] / rl, r3[1] / rl);
        let side = (0.5 * (lf[0] + rf[0]) - out.pos[0]) * rx + (0.5 * (lf[1] + rf[1]) - out.pos[1]) * ry;
        out.pos[0] += side * rx;
        out.pos[1] += side * ry;
        out.side = side;
        out.shifted = true;
    }
    out
}
/// Step 2's body frame into `out`.
///
/// # Safety
/// The pointers hold a quaternion and three vectors; `out` a frame.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_fbt_feet_frame(rot: *const Q4, pos: *const V3, lf: *const V3, rf: *const V3, out: *mut FeetFrame) {
    // SAFETY: as documented.
    unsafe { *out = feet_frame(*rot, *pos, *lf, *rf) };
}

#[cfg(test)]
mod tests {
    //! Each rule.
    use super::*;

    #[test]
    fn feet_frame_faces_and_centres() {
        // A level gaze 30 deg left of the feet's forward (+y), the head 0.1 m left of their midpoint.
        let yaw = 30f32.to_radians();
        let rot = [0.0, 0.0, (yaw * 0.5).sin(), (yaw * 0.5).cos()];
        let f = super::feet_frame(rot, [-0.1, 0.0, 1.7], [-0.15, 0.0, 0.0], [0.15, 0.0, 0.0]);
        assert!(f.turned && f.shifted);
        assert!((f.turn + yaw).abs() < 1e-5, "turned back onto the feet: {}", f.turn);
        let forward = super::ik_rotate(f.rot, [0.0, 1.0, 0.0]);
        assert!(forward[0].abs() < 1e-5 && (forward[1] - 1.0).abs() < 1e-5);
        assert!((f.side - 0.1).abs() < 1e-5 && f.pos[0].abs() < 1e-5 && (f.pos[2] - 1.7).abs() < 1e-6, "over the midpoint");
        // Feet 5 cm apart: no turn, still centred; facing backwards keeps the player's way.
        let near = super::feet_frame(rot, [0.0, 0.0, 1.7], [-0.02, 0.0, 0.0], [0.03, 0.0, 0.0]);
        assert!(!near.turned && near.shifted && near.rot == rot);
        let back = [0.0, 0.0, 1.0, 0.0];
        let behind = super::feet_frame(back, [0.0, 0.0, 1.7], [-0.15, 0.0, 0.0], [0.15, 0.0, 0.0]);
        assert!(behind.turned && behind.turn.abs() < 1e-5, "already along the feet, facing -y");
    }
    const LEVEL: Q4 = [0.0, 0.0, 0.0, 1.0];
    #[test]
    fn grades_and_angles() {
        assert_eq!((mark(0.1, 0.1), mark(0.2, 0.1), mark(0.21, 0.1)), (2, 1, 0));
        assert_eq!(head_pitch_deg(LEVEL), 0.0);
        let down = [-(45f32.to_radians() / 2.0).sin(), 0.0, 0.0, (45f32.to_radians() / 2.0).cos()];
        assert!((head_pitch_deg(down) + 45.0).abs() < 1e-3);
        assert!((arm_elevation_deg([0.0, 0.0, 0.0], [1.0, 0.0, -1.0]) + 45.0).abs() < 1e-4);
    }
    #[test]
    fn frames() {
        // Level gaze down -z: forward (0, 0, -1), right (1, 0, 0).
        let x = cross_frame([1.0, 1.7, 2.0], LEVEL, [None, None]).unwrap();
        assert_eq!((x.fwd, x.right, x.spot, x.from), ([0.0, 0.0, -1.0], [1.0, 0.0, 0.0], [1.0, 0.0, 2.0], 0));
        // Feet 30 cm apart along -x (left at +x): their line flips to face the gaze's right.
        let x = cross_frame([1.0, 1.7, 2.0], LEVEL, [Some([0.15, 0.0, 0.0]), Some([-0.15, 0.0, 0.0])]).unwrap();
        assert_eq!((x.right, x.fwd, x.spot, x.from), ([1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 0.0, 0.0], 2));
        assert_eq!(x.off, 1.0);
        // Feet within 10 cm: the gaze's heading at their midpoint.
        let x = cross_frame([1.0, 1.7, 2.0], LEVEL, [Some([0.0, 0.0, 0.04]), Some([0.0, 0.0, -0.04])]).unwrap();
        assert_eq!((x.right, x.from), ([1.0, 0.0, 0.0], 1));
        // One foot only: from the head.
        assert_eq!(cross_frame([1.0, 1.7, 2.0], LEVEL, [Some([0.0; 3]), None]).unwrap().from, 0);
        // Looking straight down: refused.
        let down = [-(89f32.to_radians() / 2.0).sin(), 0.0, 0.0, (89f32.to_radians() / 2.0).cos()];
        assert!(cross_frame([0.0; 3], down, [None, None]).is_none());
    }
    #[test]
    fn dims() {
        let frame = Frame { spot: [0.0; 3], fwd: [0.0, 0.0, -1.0], right: [1.0, 0.0, 0.0], ..Default::default() };
        let (base, tip, bar, half) = cross_dims(&frame, 1.8, 1.6, [1.4, 1.4], [-0.2, 0.2], [0.6, 0.6]);
        assert_eq!(base, [0.0, 0.0, -1.8]);
        assert_eq!(tip, 1.8);
        assert!((bar - (1.8 - 0.2 * 1.125)).abs() < 1e-6);
        assert!((half - 0.8 * 1.125).abs() < 1e-6);
    }
    #[test]
    fn marks() {
        let x = Cross { spot: [0.0; 3], fwd: [0.0, 0.0, -1.0], right: [1.0, 0.0, 0.0], base: [0.0, 0.0, -1.8], tip_y: 1.7, bar_y: 1.4, half_span: 0.8 };
        let (mut dots, mut states) = ([[0.0; 3]; 3], [-1; 3]);
        cross_marks(&x, Some(([0.05, 1.7, 0.0], LEVEL)), [Some([-0.7, 1.4, 0.0]), Some([0.4, 1.4, 0.0])], &mut dots, &mut states);
        assert_eq!(states, [2, 2, 1]);
        assert_eq!(dots[1], [-0.7, 1.4, -1.8]);
        let (mut dots, mut states) = ([[0.0; 3]; 3], [-1; 3]);
        cross_marks(&x, Some(([0.25, 1.7, 0.0], LEVEL)), [None, Some([0.7, 1.4, 0.7])], &mut dots, &mut states);
        assert_eq!(states, [0, -1, 0]);
        let (mut dots, mut states) = ([[0.0; 3]; 3], [-1; 3]);
        cross_marks(&x, None, [Some([-0.7, 1.55, 0.0]), None], &mut dots, &mut states);
        assert_eq!(states, [-1, 1, -1]);
        // Out along the arm exactly at the mark (0.75 of the half span) counts as on it.
        let unit = Cross { half_span: 1.0, ..x };
        let (mut dots, mut states) = ([[0.0; 3]; 3], [-1; 3]);
        cross_marks(&unit, None, [None, Some([0.75, 1.4, 0.0])], &mut dots, &mut states);
        assert_eq!(states, [-1, -1, 2]);
    }
}
