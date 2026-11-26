//! The thumb on the pin (main's mods/cet/CyberpunkVRPort_TacticalBelt/grenade/pin_contact.lua and its baked
//! poses/pin_contact.lua, d6e3c2c8f .. ed36cb209): with a pin grenade in one hand, a stick click starts a 1.27 s
//! timeline in which that hand's thumb hooks the ring and yanks it -- the thumb's three joints from keys on the rest
//! pose, the four fingers bent from a track per lever position (solved against the first-person hand's skin), the
//! grenade's regrip turned and shifted, the pin's hinge turned. The pin is a constrained body the kinematic thumb tip
//! drives: it hooks when the tip's segment crosses the ring's opening inside its radius, a spring answers the axial
//! travel (with damping on the pulling speed), and it lets go at 22 N past 6 mm, launched along the ring's normal.
//! The baked data (per hand 0/1, per kind) is belt_pin_contact.json, converted from the Lua table.
use super::belt::{Q, V3};
use serde_json::Value as J;
use std::collections::HashMap;
use std::sync::OnceLock;

pub(super) const DURATION: f64 = 1.27;

/// One key of the timeline: the thumb's three joint deltas on the rest pose and the grip's turn and shift.
pub(super) struct Key {
    pub t: f64,
    pub q: [Q; 3],
    pub grip: (Q, V3),
}
/// One hand's and kind's baked contact.
pub(super) struct Profile {
    pub centre: V3,
    pub normal: V3,
    pub radius: f64,
    pub tip: V3,
    pub hinge: V3,
    pub pin_rotation: Q,
    pub original_centre: V3,
    pub idle_shift: Option<V3>,
    pub offsets: [V3; 3],
    pub rest: [Q; 3],
    pub closed_rest: [Q; 3],
    pub hold: (V3, Q),
    pub snap: Option<[Q; 2]>,
    pub keys: Vec<Key>,
    /// Per lever position: rows of [t, index MCP, PIP, middle MCP, PIP, ring MCP, PIP, little MCP, PIP] (degrees).
    pub fingers: Vec<Vec<[f64; 9]>>,
}

fn f(v: &J) -> f64 {
    v.as_f64().unwrap_or(0.0)
}
fn v3(v: &J) -> V3 {
    [f(&v[0]), f(&v[1]), f(&v[2])]
}
fn q4(v: &J) -> Q {
    [f(&v[0]), f(&v[1]), f(&v[2]), f(&v[3])]
}
fn profile_of(p: &J) -> Profile {
    let mut normal = v3(&p["normal"]);
    let n = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
    normal = normal.map(|c| c / n);
    let three = |k: &str, g: fn(&J) -> Q| [g(&p[k][0]), g(&p[k][1]), g(&p[k][2])];
    let keys = p["keys"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|k| Key { t: f(&k["t"]), q: [q4(&k["q"][0]), q4(&k["q"][1]), q4(&k["q"][2])], grip: (q4(&k["grip"][0]), v3(&k["grip"][1])) })
                .collect()
        })
        .unwrap_or_default();
    let fingers = p["fingers"]
        .as_array()
        .map(|tracks| {
            tracks
                .iter()
                .map(|t| {
                    t.as_array()
                        .map(|rows| {
                            rows.iter()
                                .map(|r| {
                                    let mut row = [0.0; 9];
                                    for (i, c) in row.iter_mut().enumerate() {
                                        *c = f(&r[i]);
                                    }
                                    row
                                })
                                .collect()
                        })
                        .unwrap_or_default()
                })
                .collect()
        })
        .unwrap_or_default();
    Profile {
        centre: v3(&p["centre"]),
        normal,
        radius: f(&p["radius"]),
        tip: v3(&p["tip"]),
        hinge: v3(&p["hinge"]),
        pin_rotation: q4(&p["pinRotation"]),
        original_centre: v3(&p["originalCentre"]),
        idle_shift: p.get("idleShift").map(v3),
        offsets: [v3(&p["offsets"][0]), v3(&p["offsets"][1]), v3(&p["offsets"][2])],
        rest: three("rest", q4),
        closed_rest: three("closedRest", q4),
        hold: (v3(&p["hold"][0]), q4(&p["hold"][1])),
        snap: p.get("snap").map(|s| [q4(&s[0]), q4(&s[1])]),
        keys,
        fingers,
    }
}
static PROFILES: OnceLock<HashMap<(usize, String), Profile>> = OnceLock::new();
/// M.profile: the hand's (0 left, 1 right) contact for the kind, if one was baked.
pub(super) fn profile(side: usize, kind: Option<&str>) -> Option<&'static Profile> {
    let all = PROFILES.get_or_init(|| {
        let data: J = serde_json::from_str(include_str!("belt_pin_contact.json")).unwrap_or(J::Null);
        let mut out = HashMap::new();
        for side in 0..2usize {
            if let Some(kinds) = data[side.to_string()].as_object() {
                for (k, p) in kinds {
                    out.insert((side, k.clone()), profile_of(p));
                }
            }
        }
        out
    });
    all.get(&(side, kind?.to_string()))
}

/// The thumb's three bones (LEFT names): their index 1..3.
pub(super) fn thumb_index(name: &str) -> Option<usize> {
    match name {
        "LeftInHandThumb" => Some(1),
        "LeftHandThumb1" => Some(2),
        "LeftHandThumb2" => Some(3),
        _ => None,
    }
}
/// The four fingers' phalanges: their column of the finger track (1..8: index, middle, ring, little; MCP then PIP)
/// and their share (the DIP follows the PIP at two thirds).
fn finger_column(name: &str) -> Option<(usize, f64)> {
    Some(match name {
        "LeftHandIndex1" => (1, 1.0),
        "LeftHandIndex2" => (2, 1.0),
        "LeftHandIndex3" => (2, 0.67),
        "LeftHandMiddle1" => (3, 1.0),
        "LeftHandMiddle2" => (4, 1.0),
        "LeftHandMiddle3" => (4, 0.67),
        "LeftHandRing1" => (5, 1.0),
        "LeftHandRing2" => (6, 1.0),
        "LeftHandRing3" => (6, 0.67),
        "LeftHandPinky1" => (7, 1.0),
        "LeftHandPinky2" => (8, 1.0),
        "LeftHandPinky3" => (8, 0.67),
        _ => return None,
    })
}

fn lerp(a: f64, b: f64, u: f64) -> f64 {
    a + (b - a) * u
}
/// The normalized lerp of two quaternions (the shorter way).
pub(super) fn nlerp(a: Q, mut b: Q, u: f64) -> Q {
    if a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3] < 0.0 {
        b = b.map(|c| -c);
    }
    let q = [lerp(a[0], b[0], u), lerp(a[1], b[1], u), lerp(a[2], b[2], u), lerp(a[3], b[3], u)];
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    q.map(|c| c / n)
}

/// M.sample: the keys either side of `t` and the smoothstep between them.
pub(super) fn sample(p: &Profile, t: f64) -> (usize, usize, f64) {
    for i in 1..p.keys.len() {
        if t <= p.keys[i].t {
            let (a, b) = (&p.keys[i - 1], &p.keys[i]);
            let u = ((t - a.t) / (b.t - a.t)).max(0.0);
            return (i - 1, i, u * u * (3.0 - 2.0 * u));
        }
    }
    let last = p.keys.len().saturating_sub(1);
    (last, last, 0.0)
}
/// M.weight: in over 0.18 s, out from 0.95 s over 0.32 s.
pub(super) fn weight(t: f64) -> f64 {
    let u = (t / 0.18).clamp(0.0, 1.0);
    let v = ((t - 0.95) / 0.32).clamp(0.0, 1.0);
    u * u * (3.0 - 2.0 * u) * (1.0 - v * v * (3.0 - 2.0 * v))
}
/// M.idleFinger: a thumb bone's rest, from open to the closed lever.
pub(super) fn idle_finger(p: &Profile, name: &str, amount: f64) -> Option<Q> {
    let i = thumb_index(name)? - 1;
    Some(nlerp(p.rest[i], p.closed_rest[i], amount))
}
/// M.snap: the tip straightening on the yank, 0..1 over the time since the ring let go (rise, hold, settle).
pub(super) fn snap(since: f64) -> f64 {
    if since < 0.0 {
        return 0.0;
    }
    if since < 0.025 {
        let u = since / 0.025;
        return u * u * (3.0 - 2.0 * u);
    }
    if since < 0.095 {
        return 1.0;
    }
    let u = ((since - 0.095) / 0.22).min(1.0);
    1.0 - u * u * (3.0 - 2.0 * u)
}
/// fingerAngles: the eight flexions at `t`, linear in time and between the lever's tracks.
fn finger_angles(p: &Profile, t: f64, lever: f64) -> [f64; 8] {
    let tracks = &p.fingers;
    let n = tracks.len();
    let mut out = [0.0; 8];
    if n == 0 || tracks[0].len() < 2 {
        return out;
    }
    let x = lever.clamp(0.0, 1.0) * (n as f64 - 1.0);
    let i = ((x.floor() as i64).min(n as i64 - 2)).max(0) as usize;
    let v = if n > 1 { x - i as f64 } else { 0.0 };
    let (a, b) = (&tracks[i], &tracks[(i + 1).min(n - 1)]);
    let step = a[1][0] - a[0][0];
    // Lua's 1-based j, clamped to 1..#a-1: here 0-based 0..len-2
    let j = (((t / step).floor() as i64 + 1).clamp(1, a.len() as i64 - 1) - 1) as usize;
    let u = ((t - a[j][0]) / (a[j + 1][0] - a[j][0])).clamp(0.0, 1.0);
    let (ra, rb) = (&a[j], &a[j + 1]);
    let (sa, sb) = (&b[j.min(b.len() - 2)], &b[(j + 1).min(b.len() - 1)]);
    for k in 0..8 {
        let p0 = ra[k + 1] + (rb[k + 1] - ra[k + 1]) * u;
        let q0 = sa[k + 1] + (sb[k + 1] - sa[k + 1]) * u;
        out[k] = p0 + (q0 - p0) * v;
    }
    out
}
/// M.finger: the holding hand's keyed bone at timeline time `t` -- the thumb's three from the keys (its tip
/// straightening after the release), the fingers bent from the track by `lever`; None for any other bone.
#[allow(clippy::too_many_arguments)]
pub(super) fn finger(p: &Profile, t: f64, name: &str, x: Q, a: usize, b: usize, u: f64, released_at: Option<f64>, lever: f64) -> Option<Q> {
    let Some(index) = thumb_index(name) else {
        let (column, share) = finger_column(name)?;
        if p.fingers.is_empty() {
            return None;
        }
        let deg = finger_angles(p, t, lever)[column - 1] * share;
        if deg == 0.0 {
            return Some(x);
        }
        // q * a rotation about local Z by -deg (flexion)
        let half = -deg.to_radians() * 0.5;
        let (sz, cz) = (half.sin(), half.cos());
        let [qx, qy, qz, qw] = x;
        return Some([qx * cz + qy * sz, qy * cz - qx * sz, qw * sz + qz * cz, qw * cz - qz * sz]);
    };
    let w = weight(t);
    let (pa, pb) = (p.keys[a].q[index - 1], p.keys[b].q[index - 1]);
    let [i, j, k, r] = nlerp(pa, pb, u);
    let q = p.rest[index - 1];
    let target = [
        q[3] * i + q[0] * r + q[1] * k - q[2] * j,
        q[3] * j + q[1] * r + q[2] * i - q[0] * k,
        q[3] * k + q[2] * r + q[0] * j - q[1] * i,
        q[3] * r - q[0] * i - q[1] * j - q[2] * k,
    ];
    let mut out = nlerp(x, target, w);
    if index > 1 {
        if let (Some(snap_q), Some(at)) = (p.snap, released_at) {
            let s = snap(t - at) * w;
            if s > 0.0 {
                out = nlerp(out, snap_q[index - 2], s);
            }
        }
    }
    Some(out)
}
/// M.grip: the regrip's turn and shift between two keys.
pub(super) fn grip(p: &Profile, a: usize, b: usize, u: f64) -> (Q, V3) {
    let (ka, kb) = (&p.keys[a].grip, &p.keys[b].grip);
    (nlerp(ka.0, kb.0, u), [lerp(ka.1[0], kb.1[0], u), lerp(ka.1[1], kb.1[1], u), lerp(ka.1[2], kb.1[2], u)])
}
/// M.blendGrip: the hold blended toward the regrip by the timeline's weight.
pub(super) fn blend_grip(t: f64, from: (V3, Q), to: (V3, Q)) -> (V3, Q) {
    let u = weight(t);
    (
        [lerp(from.0[0], to.0[0], u), lerp(from.0[1], to.0[1], u), lerp(from.0[2], to.0[2], u)],
        nlerp(from.1, to.1, u),
    )
}
/// M.pinRotation: the pin turned on its hinge over the first 0.22 s.
pub(super) fn pin_rotation(p: &Profile, t: f64) -> Q {
    let u = (t / 0.22).clamp(0.0, 1.0);
    nlerp([0.0, 0.0, 0.0, 1.0], p.pin_rotation, u * u * (3.0 - 2.0 * u))
}
fn tip_radius(along: f64) -> f64 {
    if along >= 0.95 {
        return 0.0062 - (along - 0.95) * 0.034;
    }
    if along >= 0.85 {
        return 0.0076 - (along - 0.85) * 0.014;
    }
    0.0085
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Phase {
    Approach,
    Hooked,
    Tension,
    Released,
}
/// The pin's constrained state (M.new).
#[derive(Clone, Debug)]
pub(super) struct Contact {
    pub phase: Phase,
    pub force: f64,
    pub pull: f64,
    pub offset: V3,
    pub along: Option<f64>,
    origin: V3,
    pub feedback: bool,
    pub released_at: Option<f64>,
    pub velocity: V3,
}
impl Contact {
    pub(super) fn new() -> Self {
        Contact { phase: Phase::Approach, force: 0.0, pull: 0.0, offset: [0.0; 3], along: None, origin: [0.0; 3], feedback: false, released_at: None, velocity: [0.0; 3] }
    }
}
/// M.step: the kinematic tip segment (`a` the thumb's last joint, `b` its tip, in the item's frame) drives the pin;
/// true on the frame it lets go. No elapsed-time condition can detach an untouched pin.
pub(super) fn step(s: &mut Contact, p: &Profile, a: V3, b: V3, dt: f64) -> bool {
    if s.phase == Phase::Released {
        return false;
    }
    let (c, n) = (p.centre, p.normal);
    let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let denominator = d[0] * n[0] + d[1] * n[1] + d[2] * n[2];
    let Some(along) = s.along else {
        if denominator > -0.003 {
            return false;
        }
        let u = ((c[0] - a[0]) * n[0] + (c[1] - a[1]) * n[1] + (c[2] - a[2]) * n[2]) / denominator;
        if !(0.65..=0.985).contains(&u) {
            return false;
        }
        let x = [a[0] + d[0] * u, a[1] + d[1] * u, a[2] + d[2] * u];
        let length = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        let radius = p.radius - tip_radius(u) * length / denominator.abs();
        if radius < 0.0003 {
            return false;
        }
        if (x[0] - c[0]).powi(2) + (x[1] - c[1]).powi(2) + (x[2] - c[2]).powi(2) > radius * radius {
            return false;
        }
        (s.along, s.origin, s.phase) = (Some(u), x, Phase::Hooked);
        return false;
    };
    let x = [a[0] + d[0] * along - s.origin[0], a[1] + d[1] * along - s.origin[1], a[2] + d[2] * along - s.origin[2]];
    let pull = (x[0] * n[0] + x[1] * n[1] + x[2] * n[2]).max(0.0);
    let speed = (pull - s.pull) / dt.max(0.001);
    // Losing the opening releases the finger, during the loading too.
    let radial = (x[0] - pull * n[0]).powi(2) + (x[1] - pull * n[1]).powi(2) + (x[2] - pull * n[2]).powi(2);
    if radial > (p.radius - tip_radius(along)).powi(2) {
        (s.along, s.phase) = (None, Phase::Approach);
        (s.force, s.pull) = (0.0, 0.0);
        (s.offset, s.feedback) = ([0.0; 3], false);
        return false;
    }
    s.force = (1500.0 * pull + 1.5 * speed).max(0.0);
    s.pull = pull;
    s.offset = [n[0] * pull, n[1] * pull, n[2] * pull];
    s.phase = if pull > 0.0005 { Phase::Tension } else { Phase::Hooked };
    if s.force >= 22.0 && pull >= 0.006 {
        s.phase = Phase::Released;
        let launch = ((1500.0f64 / 0.04).sqrt() * pull).clamp(1.6, 3.0);
        s.velocity = [n[0] * launch, n[1] * launch, n[2] * launch];
        return true;
    }
    false
}

// ---- the ring around the index finger (ring_flick.lua and its baked clip: additive local rotations x, y, z, w)
/// The clip's joints (LEFT names) and their first column in a key row (after its time).
const FLICK_JOINTS: [(&str, usize); 8] = [
    ("LeftInHandThumb", 1),
    ("LeftHandThumb1", 5),
    ("LeftHandThumb2", 9),
    ("LeftHandIndex1", 13),
    ("LeftHandIndex2", 17),
    ("LeftHandIndex3", 21),
    ("LeftHandMiddle2", 25),
    ("LeftHandRing2", 29),
];
type FlickKey = [f64; 33];
#[rustfmt::skip]
const FLICK: [FlickKey; 10] = [
    [0.0000000, 0.0000000, 0.0000000, 0.0000000, 1.0000000, 0.0000000, 0.0000000, 0.0000000, 1.0000000, 0.0000000, 0.0000000, 0.0000000, 1.0000000, 0.0000000, 0.0000000, 0.0000000, 1.0000000, 0.0000000, 0.0000000, 0.0000000, 1.0000000, 0.0000000, 0.0000000, 0.0000000, 1.0000000, 0.0000000, 0.0000000, 0.0000000, 1.0000000, 0.0000000, 0.0000000, 0.0000000, 1.0000000],
    [0.1000000, 0.0021306, 0.0610114, 0.0348344, 0.9975268, 0.0000000, 0.0000000, 0.0871557, 0.9961947, 0.0000000, 0.0000000, 0.1218693, 0.9925462, 0.0000000, 0.0000000, 0.0174524, 0.9998477, -0.0000000, 0.0000000, -0.0261769, 0.9996573, -0.0000000, 0.0000000, -0.0174524, 0.9998477, 0.0000000, 0.0000000, 0.0348995, 0.9993908, 0.0000000, 0.0000000, 0.0174524, 0.9998477],
    [0.2800000, 0.0172522, 0.1641435, 0.1030949, 0.9808826, 0.0000000, 0.0000000, 0.2079117, 0.9781476, 0.0000000, 0.0000000, 0.1478094, 0.9890159, -0.0000000, 0.0000000, -0.0348995, 0.9993908, -0.0000000, 0.0000000, -0.0784591, 0.9969173, -0.0000000, 0.0000000, -0.0436194, 0.9990482, 0.0000000, 0.0000000, 0.0697565, 0.9975641, 0.0000000, 0.0000000, 0.0436194, 0.9990482],
    [0.4100000, 0.0289357, 0.2058883, 0.1361318, 0.9686283, 0.0000000, 0.0000000, 0.1218693, 0.9925462, -0.0000000, 0.0000000, -0.0697565, 0.9975641, -0.0000000, 0.0000000, -0.0697565, 0.9975641, -0.0000000, 0.0000000, -0.1132032, 0.9935719, -0.0000000, 0.0000000, -0.0784591, 0.9969173, -0.0000000, 0.0000000, -0.0261769, 0.9996573, -0.0000000, 0.0000000, -0.0174524, 0.9998477],
    [0.5000000, 0.0201142, 0.1638174, 0.1201980, 0.9789340, 0.0000000, 0.0000000, 0.0174524, 0.9998477, -0.0000000, 0.0000000, -0.1822355, 0.9832549, -0.0000000, 0.0000000, -0.0871557, 0.9961947, -0.0000000, 0.0000000, -0.1305262, 0.9914449, -0.0000000, 0.0000000, -0.0871557, 0.9961947, -0.0000000, 0.0000000, -0.0697565, 0.9975641, -0.0000000, 0.0000000, -0.0436194, 0.9990482],
    [0.5950000, 0.0177089, 0.1554289, 0.1118095, 0.9813393, 0.0000000, 0.0000000, 0.0087265, 0.9999619, -0.0000000, 0.0000000, -0.1993679, 0.9799247, -0.0000000, 0.0000000, -0.0958458, 0.9953962, -0.0000000, 0.0000000, -0.1391731, 0.9902681, -0.0000000, 0.0000000, -0.0958458, 0.9953962, -0.0000000, 0.0000000, -0.0784591, 0.9969173, -0.0000000, 0.0000000, -0.0523360, 0.9986295],
    [0.6450000, 0.0015223, -0.0348663, -0.0435928, 0.9984396, 0.0000000, 0.0000000, 0.2334454, 0.9723699, 0.0000000, 0.0000000, 0.2588190, 0.9659258, -0.0000000, 0.0000000, -0.0436194, 0.9990482, -0.0000000, 0.0000000, -0.1045285, 0.9945219, -0.0000000, 0.0000000, -0.0697565, 0.9975641, -0.0000000, 0.0000000, -0.0436194, 0.9990482, -0.0000000, 0.0000000, -0.0348995, 0.9993908],
    [0.7100000, 0.0031950, -0.0522383, -0.0609649, 0.9967669, 0.0000000, 0.0000000, 0.2164396, 0.9762960, 0.0000000, 0.0000000, 0.2840153, 0.9588197, -0.0000000, 0.0000000, -0.0348995, 0.9993908, -0.0000000, 0.0000000, -0.0958458, 0.9953962, -0.0000000, 0.0000000, -0.0610485, 0.9981348, 0.0000000, 0.0000000, 0.0000000, 1.0000000, -0.0000000, 0.0000000, -0.0087265, 0.9999619],
    [0.8700000, 0.0003046, -0.0174497, -0.0174497, 0.9996954, 0.0000000, 0.0000000, 0.0958458, 0.9953962, 0.0000000, 0.0000000, 0.1218693, 0.9925462, -0.0000000, 0.0000000, -0.0174524, 0.9998477, -0.0000000, 0.0000000, -0.0436194, 0.9990482, -0.0000000, 0.0000000, -0.0261769, 0.9996573, 0.0000000, 0.0000000, 0.0174524, 0.9998477, 0.0000000, 0.0000000, 0.0087265, 0.9999619],
    [1.1000000, 0.0000000, 0.0000000, 0.0000000, 1.0000000, 0.0000000, 0.0000000, 0.0000000, 1.0000000, 0.0000000, 0.0000000, 0.0000000, 1.0000000, 0.0000000, 0.0000000, 0.0000000, 1.0000000, 0.0000000, 0.0000000, 0.0000000, 1.0000000, 0.0000000, 0.0000000, 0.0000000, 1.0000000, 0.0000000, 0.0000000, 0.0000000, 1.0000000, 0.0000000, 0.0000000, 0.0000000, 1.0000000],
];
/// ring_flick's fingerKeys: the clip's keys either side of `t` and the eased fraction between them.
pub(super) fn flick_keys(t: f64) -> (usize, usize, f64) {
    for n in 1..FLICK.len() {
        if t <= FLICK[n][0] {
            let (a, b) = (FLICK[n - 1][0], FLICK[n][0]);
            let u = ((t - a) / (b - a)).clamp(0.0, 1.0);
            return (n - 1, n, u * u * (3.0 - 2.0 * u));
        }
    }
    (FLICK.len() - 1, FLICK.len() - 1, 0.0)
}
/// ring_flick's finger: the clip's additive rotation for the bone (LEFT name) laid after `x` (x * delta, normalised
/// by the delta's length); `x` unchanged for a bone the clip does not move.
pub(super) fn flick_finger(name: &str, x: Q, a: usize, b: usize, u: f64) -> Q {
    let Some(&(_, n)) = FLICK_JOINTS.iter().find(|j| j.0 == name) else { return x };
    let (ka, kb) = (&FLICK[a], &FLICK[b]);
    let i = ka[n] + (kb[n] - ka[n]) * u;
    let j = ka[n + 1] + (kb[n + 1] - ka[n + 1]) * u;
    let k = ka[n + 2] + (kb[n + 2] - ka[n + 2]) * u;
    let r = ka[n + 3] + (kb[n + 3] - ka[n + 3]) * u;
    let norm = (i * i + j * j + k * k + r * r).sqrt();
    let [qx, qy, qz, qw] = x;
    [
        (qw * i + qx * r + qy * k - qz * j) / norm,
        (qw * j + qy * r + qz * i - qx * k) / norm,
        (qw * k + qz * r + qx * j - qy * i) / norm,
        (qw * r - qx * i - qy * j - qz * k) / norm,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_baked_profiles_load() {
        for side in 0..2 {
            for kind in ["frag", "ozob", "flash", "incendiary", "biohazard", "smoke"] {
                let p = profile(side, Some(kind)).expect(kind);
                assert_eq!(p.keys.len(), 10);
                assert_eq!(p.fingers.len(), 5);
                let n = p.normal;
                assert!(((n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt() - 1.0).abs() < 1e-12);
                assert!((p.keys.last().unwrap().t - DURATION).abs() < 1e-9);
            }
        }
        assert!(profile(0, Some("emp")).is_none());
    }

    #[test]
    fn the_timeline_weights_and_the_snap() {
        assert_eq!(weight(0.0), 0.0);
        assert!((weight(0.5) - 1.0).abs() < 1e-12);
        assert!(weight(1.27).abs() < 1e-12);
        assert_eq!(snap(-0.1), 0.0);
        assert_eq!(snap(0.05), 1.0);
        assert!(snap(0.4).abs() < 1e-12);
        let p = profile(1, Some("frag")).unwrap();
        let (a, b, u) = sample(p, 0.0);
        assert_eq!((a, b), (0, 1));
        assert_eq!(u, 0.0);
        let (a, b, u) = sample(p, 5.0);
        assert_eq!((a, b, u), (9, 9, 0.0));
    }

    #[test]
    fn a_tip_through_the_ring_hooks_then_releases_on_a_yank() {
        let p = profile(1, Some("frag")).unwrap();
        let (c, n) = (p.centre, p.normal);
        let mut s = Contact::new();
        // a segment along -normal crossing the ring's centre at 0.8 of its length
        let at = |off: f64| {
            let a = [c[0] + n[0] * (0.016 + off), c[1] + n[1] * (0.016 + off), c[2] + n[2] * (0.016 + off)];
            let b = [c[0] - n[0] * (0.004 - off), c[1] - n[1] * (0.004 - off), c[2] - n[2] * (0.004 - off)];
            (a, b)
        };
        let (a, b) = at(0.0);
        assert!(!step(&mut s, p, a, b, 0.016));
        assert_eq!(s.phase, Phase::Hooked);
        // a slow pull below the threshold: tension, no release
        let (a, b) = at(0.002);
        assert!(!step(&mut s, p, a, b, 0.016));
        assert_eq!(s.phase, Phase::Tension);
        // 15 mm of pull: past 22 N and 6 mm, released along the normal
        let (a, b) = at(0.015);
        assert!(step(&mut s, p, a, b, 0.016));
        assert_eq!(s.phase, Phase::Released);
        let v = s.velocity;
        assert!(v[0] * n[0] + v[1] * n[1] + v[2] * n[2] >= 1.6 - 1e-9);
        // a segment far off the ring never hooks
        let mut s = Contact::new();
        let off = [c[0] + 0.05, c[1], c[2]];
        assert!(!step(&mut s, p, [off[0] + n[0] * 0.016, off[1] + n[1] * 0.016, off[2] + n[2] * 0.016], [off[0] - n[0] * 0.004, off[1] - n[1] * 0.004, off[2] - n[2] * 0.004], 0.016));
        assert_eq!(s.phase, Phase::Approach);
    }

    #[test]
    fn the_flick_clip() {
        assert_eq!(flick_keys(0.0), (0, 1, 0.0));
        let (a, b, u) = flick_keys(2.0);
        assert_eq!((a, b, u), (9, 9, 0.0));
        let x = [0.0, 0.0, 0.0, 1.0];
        assert_eq!(flick_finger("LeftHandPinky1", x, 1, 2, 0.5), x);
        let q = flick_finger("LeftHandThumb1", x, 0, 0, 0.0);
        assert!((q[3] - 1.0).abs() < 1e-12);
    }
}
