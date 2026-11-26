//! include/Anim/NpcJointLimits.hpp: the limits of a held person's joints -- the twist about a bone kept across the
//! quaternion seam, a swing cone and a twist range for the shoulder, the hip, the wrist and the ankle (functional limits
//! for an actively held biped, not the ragdoll's shape-joint frames), and the elbow's and the knee's hinge from the rig's
//! reference pose.
pub const PI: f32 = core::f32::consts::PI;
pub const DEG: f32 = PI / 180.0;

pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub fn cross(a: &[f32], b: &[f32]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
pub fn unit(v: &mut [f32; 3]) -> bool {
    let n = dot(v, v).sqrt();
    if !(n > 1e-7) || !n.is_finite() {
        return false;
    }
    for x in v.iter_mut() {
        *x /= n;
    }
    true
}
pub fn normalize(q: &mut [f32; 4]) -> bool {
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if !(n > 1e-7) || !n.is_finite() {
        return false;
    }
    for x in q.iter_mut() {
        *x /= n;
    }
    true
}
pub fn conjugate(q: &[f32; 4]) -> [f32; 4] {
    [-q[0], -q[1], -q[2], q[3]]
}
pub fn multiply(a: &[f32; 4], b: &[f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}
pub fn rotate(q: &[f32; 4], v: &[f32; 3]) -> [f32; 3] {
    let t = cross(q, v);
    let u = cross(q, &t);
    [v[0] + 2.0 * (q[3] * t[0] + u[0]), v[1] + 2.0 * (q[3] * t[1] + u[1]), v[2] + 2.0 * (q[3] * t[2] + u[2])]
}
pub fn axis_angle(axis: &[f32; 3], angle: f32) -> [f32; 4] {
    let s = (angle * 0.5).sin();
    [axis[0] * s, axis[1] * s, axis[2] * s, (angle * 0.5).cos()]
}
/// The turn taking direction `from` to `to` (half way round about a perpendicular for opposite ones); None for a zero
/// direction.
pub fn arc(from: &[f32; 3], to: &[f32; 3]) -> Option<[f32; 4]> {
    let (mut a, mut b) = (*from, *to);
    if !unit(&mut a) || !unit(&mut b) {
        return None;
    }
    let d = dot(&a, &b).clamp(-1.0, 1.0);
    if d < -0.99999 {
        let mut least = 0;
        for k in 1..3 {
            if a[k].abs() < a[least].abs() {
                least = k;
            }
        }
        let mut basis = [0.0f32; 3];
        basis[least] = 1.0;
        let mut c = cross(&a, &basis);
        if !unit(&mut c) {
            return None;
        }
        return Some([c[0], c[1], c[2], 0.0]);
    }
    let c = cross(&a, &b);
    let mut q = [c[0], c[1], c[2], 1.0 + d];
    normalize(&mut q).then_some(q)
}
pub fn slerp(a: &[f32; 4], b: &[f32; 4], w: f32) -> [f32; 4] {
    let mut d = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    let sign = if d < 0.0 { -1.0 } else { 1.0 };
    d = d.abs().clamp(0.0, 1.0);
    let w = w.clamp(0.0, 1.0);
    let (mut x, mut y) = (1.0 - w, w);
    if d < 0.9995 {
        let angle = d.acos();
        let s = angle.sin();
        x = ((1.0 - w) * angle).sin() / s;
        y = (w * angle).sin() / s;
    }
    let mut out = [0.0; 4];
    for k in 0..4 {
        out[k] = a[k] * x + b[k] * sign * y;
    }
    normalize(&mut out);
    out
}
/// The twist of `q` about `axis`, -pi..pi.
pub fn twist(q: &[f32; 4], axis: &[f32; 3]) -> f32 {
    let sign = if q[3] < 0.0 { -1.0 } else { 1.0 };
    2.0 * (dot(q, axis) * sign).atan2(q[3].abs())
}
/// IEEE remainder (C's std::remainder): x less the nearest whole multiple of y, ties to even.
pub fn remainder(x: f32, y: f32) -> f32 {
    let (x, y) = (f64::from(x), f64::from(y));
    (x - (x / y).round_ties_even() * y) as f32
}
/// Follow a requested rotation across the quaternion +/-180-degree seam. The limited joint must keep its current stop
/// instead of jumping to the other one.
#[derive(Clone, Copy, Default)]
pub struct TwistHistory {
    pub raw: f32,
    pub angle: f32,
    pub valid: bool,
}
impl TwistHistory {
    pub fn observe(&mut self, value: f32) {
        self.angle = if self.valid { self.angle + remainder(value - self.raw, 2.0 * PI) } else { value };
        self.raw = value;
        self.valid = true;
    }
}
/// The twist of `desired` about the bone's axis from the rig's reference frame.
pub fn reference_twist(reference: &[f32; 4], desired: &[f32; 4], bone_axis: &[f32; 3]) -> Option<f32> {
    let mut axis = *bone_axis;
    if !unit(&mut axis) {
        return None;
    }
    let mut relative = multiply(&conjugate(reference), desired);
    if !normalize(&mut relative) {
        return None;
    }
    let angle = twist(&relative, &axis);
    angle.is_finite().then_some(angle)
}
#[derive(Clone, Copy)]
pub struct Cone {
    pub y: f32,
    pub z: f32,
    pub twist: f32,
}
pub const SHOULDER: Cone = Cone { y: 110.0 * DEG, z: 130.0 * DEG, twist: 100.0 * DEG };
pub const HIP: Cone = Cone { y: 65.0 * DEG, z: 100.0 * DEG, twist: 45.0 * DEG };
pub const WRIST: Cone = Cone { y: 60.0 * DEG, z: 30.0 * DEG, twist: 80.0 * DEG };
pub const ANKLE: Cone = Cone { y: 25.0 * DEG, z: 45.0 * DEG, twist: 20.0 * DEG };
/// Clamp the rotation relative to the rig's reference local frame. Twist is about the bone, swing uses the orthogonal
/// rig axes; no world-axis dependence. `twist_reference`: the twist followed across the seam.
pub fn clamp(reference: &[f32; 4], desired: &[f32; 4], bone_axis: &[f32; 3], limit: Cone, twist_reference: Option<f32>) -> Option<[f32; 4]> {
    let mut axis = *bone_axis;
    if !unit(&mut axis) {
        return None;
    }
    let mut q = multiply(&conjugate(reference), desired);
    if !normalize(&mut q) {
        return None;
    }
    if q[3] < 0.0 {
        q = q.map(|v| -v);
    }
    let parallel = axis[2];
    let mut z = [-parallel * axis[0], -parallel * axis[1], 1.0 - parallel * axis[2]];
    if !unit(&mut z) {
        return None;
    }
    let y = cross(&z, &axis);
    let angle = twist(&q, &axis);
    let tw = axis_angle(&axis, angle);
    let mut swing = multiply(&q, &conjugate(&tw));
    normalize(&mut swing);
    if swing[3] < 0.0 {
        swing = swing.map(|v| -v);
    }
    let length = dot(&swing, &swing).sqrt();
    if length > 1e-7 {
        let turn = 2.0 * length.atan2(swing[3].max(0.0));
        let sy = dot(&swing, &y) / length * turn;
        let sz = dot(&swing, &z) / length * turn;
        let over = (sy / limit.y).hypot(sz / limit.z);
        let swing_axis = [swing[0] / length, swing[1] / length, swing[2] / length];
        swing = axis_angle(&swing_axis, turn / over.max(1.0));
    }
    let continuous = match twist_reference {
        Some(r) => r + remainder(angle - r, 2.0 * PI),
        None => angle,
    };
    let tw = axis_angle(&axis, continuous.clamp(-limit.twist, limit.twist));
    let mut out = multiply(reference, &multiply(&swing, &tw));
    normalize(&mut out).then_some(out)
}
/// The elbow's or the knee's hinge: its reference rotation, its axis (the reference's Z), the bones in and out of it,
/// its reference angle, which way it bends, how far and how far it twists.
#[derive(Clone, Copy)]
pub struct Hinge {
    pub rest: [f32; 4],
    pub axis: [f32; 3],
    pub incoming: [f32; 3],
    pub outgoing: [f32; 3],
    pub rest_angle: f32,
    pub sign: f32,
    pub minimum: f32,
    pub maximum: f32,
    pub twist: f32,
}
impl Default for Hinge {
    fn default() -> Hinge {
        Hinge { rest: [0.0, 0.0, 0.0, 1.0], axis: [0.0, 0.0, 1.0], incoming: [1.0, 0.0, 0.0], outgoing: [1.0, 0.0, 0.0], rest_angle: 0.0, sign: -1.0, minimum: 2.0 * DEG, maximum: 145.0 * DEG, twist: 80.0 * DEG }
    }
}
/// The hinge from the rig: humanoid rig hinge axes are Z (also authored by its foot IK setups); unknown axes are
/// rejected instead of bending a different skeleton wrong. NPC metarigs commonly use the authored A-pose (about 40
/// degrees of elbow flexion), while the player/reference rig may be almost straight: that signed reference angle is
/// kept, only a degenerate fold is ambiguous.
pub fn configure(h: &mut Hinge, reference: &[f32; 4], incoming: &[f32; 3], outgoing: &[f32; 3], leg: bool) -> bool {
    h.rest = *reference;
    if !normalize(&mut h.rest) {
        return false;
    }
    h.outgoing = *outgoing;
    if !unit(&mut h.outgoing) {
        return false;
    }
    h.axis = rotate(&h.rest, &[0.0, 0.0, 1.0]);
    let mut a = *incoming;
    if !unit(&mut a) {
        return false;
    }
    h.incoming = a;
    let b = rotate(&h.rest, &h.outgoing);
    let c = cross(&a, &b);
    if dot(&a, &h.axis).abs() > 0.05 || dot(&b, &h.axis).abs() > 0.05 || dot(&a, &b) < -0.985 {
        return false;
    }
    h.rest_angle = dot(&c, &h.axis).atan2(dot(&a, &b));
    h.sign = if leg { 1.0 } else { -1.0 };
    h.maximum = (if leg { 150.0 } else { 145.0 }) * DEG;
    h.twist = (if leg { 3.0 } else { 80.0 }) * DEG;
    true
}
/// The hinge bent `flex` with the animation's pronation kept within its twist.
pub fn pose(h: &Hinge, flex: f32, animated: &[f32; 4], twist_reference: Option<f32>) -> [f32; 4] {
    let relative = multiply(&conjugate(&h.rest), animated);
    let raw = twist(&relative, &h.outgoing);
    let continuous = match twist_reference {
        Some(r) => r + remainder(raw - r, 2.0 * PI),
        None => raw,
    };
    let pronation = continuous.clamp(-h.twist, h.twist);
    let bend = axis_angle(&h.axis, h.sign * flex.clamp(h.minimum, h.maximum) - h.rest_angle);
    let base = multiply(&bend, &h.rest);
    let tw = axis_angle(&h.outgoing, pronation);
    let mut out = multiply(&base, &tw);
    normalize(&mut out);
    out
}
