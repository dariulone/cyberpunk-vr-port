//! GRASP (main's src/Anim/GraspPose.cpp): the fingers closed on what a hand holds, from the thing's own shape and size --
//! not a recorded pose.
//!
//! A thing is a primitive -- a box, a ball, a cylinder, a cone or (a person's part) a capsule, by its half sizes -- at a
//! pose in the hand's weapon slot. Its shape and size make the kind of grasp (classify):
//!  - WRAP: a can, a bottle, a cone, a bar: across the palm where the fingers start, a little diagonal, the fingers
//!    curled round it and the thumb over it;
//!  - BALL: the palm under it, the fingers spread round it with their pads on it, the thumb opposite them;
//!  - EDGE: a plate: held from below by the middle of its short edge, the four fingers up one cover and the thumb on the
//!    other;
//!  - FLAT: anything bigger -- a box: the face nearest the palm onto it, held where the hand took it;
//!  - PINCH: a tiny thing (no half size of 1.2 cm): flat on the pad of the index finger bent a little, the thumb's pad on
//!    its other side;
//!  - TRIPOD: a small thing (a car key's fob: no half size of 4.5 cm, the middle one under 2.2 cm): between the thumb's
//!    pad and the pads of the index and middle fingers side by side; ring and little finger curled out of the way. A
//!    plate too thick for a cover's grip (4-6 cm) the same way by its side, like a crab's claw.
//! A long thing (a can, a bottle, a bar) is taken the way the hand came to it (approach), not by its shape alone:
//!  - in its middle, or from its side: WRAP;
//!  - TOP: the palm over one of its ends, facing along it, that end not the lower one: the fingertips round its rim;
//!  - CRADLE: the palm under its lower end, facing up along it (caught from below): it stands on the palm.
//! Which end is lower is the world's: the model's up in the hand's frame, from the drawn hand of the frame.
//! A PART of something bigger (grasp_part_of: a bottle's neck, or its body below the shoulder) is wrapped however thin
//! it is, and only its free ends are taken from above or below.
//!
//! SEAT (VRGraspSeat, grasp_take): where in the slot the thing lies for its grasp, against the palm measured from the
//! skeleton every pass in its reference (open) pose. CLOSE: the four fingers from the reference pose, each joint turned
//! about its own hinge a step at a time until a segment comes into the thing or the palm; the thumb searched for the
//! pose whose pad lies on the thing where the grasp wants it. Fingers are capsules off their bones toward the pads (the
//! game's first-person hand measured round the rig's bones). Everything is in the HAND bone's frame.
//!
//! BAKED GRASPS (baked.rs): per cooked prop mesh, grasps fitted offline on its drawn surface. A PERSON'S PART
//! (grasp_person): a capsule closed on by a style, eased in and out and re-solved as it moves in the hand.
#![allow(clippy::too_many_arguments)]
use super::env::*;
use super::rig::{
    VRIK_LadderFingerBone, VRIK_QuatConj, VRIK_QuatFromTo, VRIK_QuatMul, VRIK_QuatNorm, VRIK_QuatRotateVec, VRIK_ReferenceFingerRotation, VRIK_ReferenceLocal,
};
use super::*;
use cvr_body::cmath::{clamp, cos, hypot, max, min, sin, sqrt};
use std::sync::{Mutex, MutexGuard};

mod baked;
mod close;
mod natives;
mod person;
mod seat;

pub use baked::{grasp_baked_has, grasp_load_baked, grasp_take_baked, BakedTake};
pub use natives::*;
pub use person::{grasp_ease, grasp_let_go, grasp_part_of, grasp_person, grasp_person_gap};
pub use seat::{cvr_anim_grasp_fingers, cvr_anim_grasp_holding, grasp_holding, grasp_take};

extern "C" {
    static mut g_VRSmokeCigIdx: i32;
    static mut g_VRSmokeLighterIdx: i32;
    // Solves, passes that wrote a grasp, palms measured, segments that touched in the last solve.
    static mut CyberpunkVR_DebugGrasp: [u64; 4];
    // Per side, the last solve: its kind (0 flat, 1 wrap, 2 ball, 3 pinch, 4 edge, 5 top, 6 cradle, 7 tripod), the
    // thumb's metacarpal elevation and angle out and its two joints, then index..pinky's three joints each, in degrees.
    static mut CyberpunkVR_DebugGraspAngles: [[f32; 17]; 2];
    static mut CyberpunkVR_DebugGraspPalm: [[f32; 12]; 2];
    static mut CyberpunkVR_DebugGraspNormalFlip: [i32; 2];
    // Per side, the thumb's search: its cost, the overlap (mm), the pad's gap to the thing (mm), the pad from its goal.
    static mut CyberpunkVR_DebugGraspThumb: [[f32; 4]; 2];
}
/// Per side, the last approach on a long thing: the palm's normal against its axis (|cos|), the palm along it from its
/// middle (share of the half length, signed), the end the palm is at against the world's up (cos; 9 unknown), the kind
/// it gave.
#[no_mangle]
pub static mut CyberpunkVR_DebugGraspApproach: [[f32; 4]; 2] = [[0.0; 4]; 2];
/// Fingers closed again capped by the finger beside them (the hand's coupling), since load.
#[no_mangle]
pub static mut CyberpunkVR_DebugGraspCoupled: u64 = 0;
/// The last approach's read-out for a side (HeldBody's log).
pub fn debug_grasp_approach(side: usize) -> [f32; 4] {
    // SAFETY: a debug read-out, written by the grasp's own calls.
    unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_DebugGraspApproach[side.min(1)]) }
}

pub(crate) const K_BOX: i32 = 0;
pub(crate) const K_SPHERE: i32 = 1;
pub(crate) const K_CYLINDER: i32 = 2;
pub(crate) const K_CONE: i32 = 3;
pub(crate) const K_CAPSULE: i32 = 4;
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Kind {
    Flat = 0,
    Wrap = 1,
    Ball = 2,
    Pinch = 3,
    Edge = 4,
    Top = 5,
    Cradle = 6,
    Tripod = 7,
}
impl Kind {
    fn from_i32(k: i32) -> Kind {
        match k {
            1 => Kind::Wrap,
            2 => Kind::Ball,
            3 => Kind::Pinch,
            4 => Kind::Edge,
            5 => Kind::Top,
            6 => Kind::Cradle,
            7 => Kind::Tripod,
            _ => Kind::Flat,
        }
    }
}
/// A glass's stem (no free end: the waist between the bowl and the foot) is WRAPPED from below, the bowl over the index
/// finger -- not pinched between straight fingers. GRASP_STEM_PINCH (the harness) brings the pinch back for comparison.
fn precision_stem(shape: i32, half: &V3, ends: i32, part: bool) -> bool {
    static PINCH: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let pinch = *PINCH.get_or_init(|| std::env::var_os("GRASP_STEM_PINCH").is_some());
    pinch && part && ends == 0 && shape == K_CYLINDER && max(half[0], half[1]) <= 0.01
}
const SLOTS: usize = 19;
const K_PI: f32 = 3.141_592_65;
const K_DEG: f32 = K_PI / 180.0;
/// the palm's skin over the plane of the hand's bones (m)
const PALM_SKIN: f32 = 0.014;
/// a box no half size of which reaches this is pinched (m)
const PINCH_SIZE: f32 = 0.012;
/// a ball under this radius is pinched (m)
const PINCH_BALL: f32 = 0.012;
/// a thing no half size of which passes this...
const TRIPOD_LONG: f32 = 0.045;
/// ...and whose middle half size (a ball's or a rod's radius) is under this: three fingers (m)
const TRIPOD_MID: f32 = 0.022;
/// a plate this thick (half) and up to...
const SIDE_PINCH: f32 = 0.020;
/// ...this: taken by its side in three fingers (m)
const SIDE_PINCH_MAX: f32 = 0.030;
/// three fingers' pads this far in from a bigger thing's edge (m)...
const TRI_INSET: f32 = 0.012;
/// ...and its end on the little finger's side this far past them: over the middle finger
const TRI_SIDE: f32 = 0.012;
/// a long thing taken by an end: the palm's normal this near its axis (cosine)...
const END_ON: f32 = 0.70;
/// ...and the palm at least this share of its half length along it from its middle
const END_NEAR: f32 = 0.55;
/// that end lower than this (cosine with the world's down): caught from below
const END_DOWN: f32 = 0.35;
/// taken from above: its end this far under the palm's skin (m)
const TOP_GAP: f32 = 0.014;
/// ...and its middle this far toward the fingers from the palm's (m)
const TOP_AHEAD: f32 = 0.012;
/// caught from below: its end this far off the palm's skin (m)
const CRADLE_RISE: f32 = 0.002;
/// the index finger's pad over its bone (m)
const PINCH_PAD: f32 = 0.0085;
/// a box is a bar when its two smaller half sizes are within this (m)
const WRAP_THICK: f32 = 0.042;
/// a cylinder or a cone up to this radius is wrapped (m)
const WRAP_RADIUS: f32 = 0.05;
/// a wrapped thing's axis behind the knuckles (m)
const WRAP_BACK: f32 = 0.008;
const WRAP_TILT: f32 = 14.0 * K_DEG;
/// Thick limbs lie more diagonally across the hand, but their axis stays behind the knuckles. Advancing it by 30 mm
/// flattened the fingers against its underside instead of letting them wrap.
const PART_THIN: f32 = 0.04;
const PART_THICK: f32 = 0.07;
const PART_TILT_THICK: f32 = 32.0 * K_DEG;
/// a wrapped thing's thumb-side end at least this far past the index knuckle (m)
const WRAP_OVERHANG: f32 = 0.055;
/// the palm's centre kept this far inside a held face's edge (m)
const FLAT_MARGIN: f32 = 0.025;
/// A face or a ball lies on the palm's pads (the thenar, the hypothenar) this far over its middle's skin (m).
const PAD_RISE: f32 = 0.006;
/// a face the palm lies on this far over the palm's plane: on the palm's pads (the real palm stands 5-9 mm over the
/// plane at its middle, 20-30 mm at the thenar), not through them
const FLAT_RISE: f32 = 0.011;
/// a box is a plate when its thinnest half size is under this share of the next
const PLATE: f32 = 0.45;
/// A held plate pinched like a crab's claw: its near cover EDGE_GAP past the knuckles' joints (under 0: behind them), its
/// edge EDGE_LIFT off the palm -- the fingers curled onto one cover, the thumb bent onto the other, the plate between
/// the pads.
const EDGE_GAP: f32 = -0.008;
const EDGE_LIFT: f32 = 0.012;
/// a held face's far edge, the fingers hooked over it, this far past the index knuckle (m)
const FLAT_HOOK: f32 = 0.006;
/// ...when the edge is no further than this past it (the fingers reach over it)
const FLAT_REACH: f32 = 0.075;
const STEP: f32 = 1.5 * K_DEG;
/// a finger joint's pace while the joint before it still closes
const FOLLOW: f32 = 0.2;
/// How much more a finger's joint may be bent than the same joint of the finger beside it, degrees, per pair
/// (index-middle, middle-ring, ring-little) and joint (MCP, PIP, DIP). The middle, ring and little fingers share the deep
/// flexor and are tied by the tendons between them, the ring and the little finger the most.
const COUPLE_DEG: [[f32; 3]; 3] = [[50.0, 35.0, 30.0], [35.0, 20.0, 15.0], [30.0, 10.0, 8.0]];
/// a person's part: the grasp eased in, and out (s)
const PERSON_IN: f32 = 0.12;
const PERSON_OUT: f32 = 0.18;
/// ...and from one solve to the next (s)
const PERSON_TURN_TAU: f32 = 0.08;
/// re-solved once the part is this far off its seat (m)...
const PERSON_MOVED: f32 = 0.025;
/// ...or turned this far from it...
const PERSON_TURNED: f32 = 20.0 * K_DEG;
/// ...for this long (s)
const PERSON_EVERY: f64 = 0.5;
/// a part held round is turned at most this far onto its seat...
const SEAT_TURN_ROUND: f32 = 45.0 * K_DEG;
/// ...a palm on one turned at most this far to lie flat on it
const SEAT_TURN_PALM: f32 = 30.0 * K_DEG;
/// the palm's plane this far off a part it lies on: on its pads (m)
const SEAT_PALM_GAP: f32 = 0.007;
/// a seat takes the hand this far at the most (m)
const SEAT_MOST: f32 = 0.06;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Pose {
    p: V3,
    q: Q,
}
impl Default for Pose {
    fn default() -> Self {
        POSE0
    }
}
const POSE0: Pose = Pose { p: [0.0; 3], q: ID };

fn sub(a: &V3, b: &V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn mad(a: &V3, b: &V3, s: f32) -> V3 {
    [a[0] + b[0] * s, a[1] + b[1] * s, a[2] + b[2] * s]
}
fn dot(a: &V3, b: &V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn len(a: &V3) -> f32 {
    sqrt(dot(a, a))
}
fn cross(a: &V3, b: &V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn unit(a: &mut V3) -> bool {
    let l = len(a);
    if !(l > 1e-8) {
        return false;
    }
    for v in a.iter_mut() {
        *v /= l;
    }
    true
}
fn neg(a: &V3) -> V3 {
    [-a[0], -a[1], -a[2]]
}
fn rot(q: &Q, v: &V3) -> V3 {
    let mut o = [0.0f32; 3];
    // SAFETY: three and four floats.
    unsafe { VRIK_QuatRotateVec(q.as_ptr(), v.as_ptr(), o.as_mut_ptr()) };
    o
}
fn conj(q: &Q) -> Q {
    let mut o = [0.0f32; 4];
    // SAFETY: four floats each.
    unsafe { VRIK_QuatConj(q.as_ptr(), o.as_mut_ptr()) };
    o
}
fn unrot(q: &Q, v: &V3) -> V3 {
    rot(&conj(q), v)
}
fn qmul(a: &Q, b: &Q) -> Q {
    let mut o = [0.0f32; 4];
    // SAFETY: four floats each.
    unsafe { VRIK_QuatMul(a.as_ptr(), b.as_ptr(), o.as_mut_ptr()) };
    o
}
fn qnorm(q: &mut Q) {
    // SAFETY: four floats.
    unsafe { VRIK_QuatNorm(q.as_mut_ptr()) };
}
fn from_to(a: &V3, b: &V3) -> Q {
    let mut o = [0.0f32; 4];
    // SAFETY: three, three and four floats.
    unsafe { VRIK_QuatFromTo(a.as_ptr(), b.as_ptr(), o.as_mut_ptr()) };
    o
}
fn compose(a: &Pose, b: &Pose) -> Pose {
    let r = rot(&a.q, &b.p);
    let mut o = POSE0;
    for k in 0..3 {
        o.p[k] = a.p[k] + r[k];
    }
    o.q = qmul(&a.q, &b.q);
    qnorm(&mut o.q);
    o
}
/// A point into frame f's own coordinates.
fn to_frame(f: &Pose, p: &V3) -> V3 {
    unrot(&f.q, &sub(p, &f.p))
}
fn axis_angle(axis: &V3, rad: f32) -> Q {
    let s = sin(rad * 0.5);
    [axis[0] * s, axis[1] * s, axis[2] * s, cos(rad * 0.5)]
}
fn e_axis(k: usize) -> V3 {
    let mut e = [0.0f32; 3];
    e[k] = 1.0;
    e
}
/// The bone axis (in a frame turned by q) nearest the world axis k, when it is near enough (an anatomical hinge), else
/// k itself, in q's own coordinates.
fn local_hinge(q: &Q, k: &V3) -> V3 {
    let mut out = unrot(q, k);
    let mut best = 0usize;
    for a in 1..3 {
        if out[a].abs() > out[best].abs() {
            best = a;
        }
    }
    if out[best].abs() > 0.85 {
        let s = if out[best] > 0.0 { 1.0 } else { -1.0 };
        out = [0.0; 3];
        out[best] = s;
    }
    unit(&mut out);
    out
}

unsafe fn local(buf: *const u8, bone: i32) -> Pose {
    let base = buf.add(bone as usize * 48);
    let t = base as *const f32;
    let q = base.add(16) as *const f32;
    let mut o = Pose { p: [*t, *t.add(1), *t.add(2)], q: [*q, *q.add(1), *q.add(2), *q.add(3)] };
    qnorm(&mut o.q);
    o
}

unsafe fn valid_bone(b: i32) -> bool {
    b >= 0 && b < g_VRBoneCount && b < MAXB
}

/// Bone b in the frame of its ancestor `root` through the buffer's locals, a finger slot's rotation replaced by
/// `rot[slot]` when given. None when root is not an ancestor.
unsafe fn in_frame(buf: *const u8, side: i32, bone: i32, root: i32, rotations: Option<&[Q; SLOTS]>) -> Option<Pose> {
    let mut chain = [0i32; 32];
    let mut n = 0usize;
    let mut b = bone;
    while b != root {
        if !valid_bone(b) || n >= 32 {
            return None;
        }
        chain[n] = b;
        n += 1;
        b = g_VRBoneParent[b as usize] as i32;
    }
    let mut acc = POSE0;
    for i in (0..n).rev() {
        let mut l = local(buf, chain[i]);
        if let Some(r) = rotations {
            for (s, rs) in r.iter().enumerate() {
                if VRIK_LadderFingerBone(side, s as i32) == chain[i] {
                    l.q = *rs;
                    break;
                }
            }
        }
        acc = compose(&acc, &l);
    }
    Some(acc)
}

/// The rig's reference rotations (the open hand), the buffer's where the reference has none.
unsafe fn reference_fingers(buf: *const u8, side: i32, start: &mut [Q; SLOTS], mut have: Option<&mut [bool; SLOTS]>) {
    for s in 0..SLOTS {
        let b = VRIK_LadderFingerBone(side, s as i32);
        if let Some(h) = have.as_deref_mut() {
            h[s] = false;
        }
        if !valid_bone(b) {
            start[s] = ID;
            continue;
        }
        if !VRIK_ReferenceFingerRotation(b, start[s].as_mut_ptr()) {
            start[s] = local(buf, b).q;
        }
        if let Some(h) = have.as_deref_mut() {
            h[s] = true;
        }
    }
}

/// The fingertip in its distal bone's frame: the bone after it, or the distal bone's own reach again.
unsafe fn tip_offset(buf: *const u8, distal: i32) -> V3 {
    let mut i = 0i32;
    while i < g_VRBoneCount && i < MAXB {
        if g_VRBoneParent[i as usize] as i32 == distal {
            let c = local(buf, i);
            if len(&c.p) > 0.004 {
                return c.p;
            }
        }
        i += 1;
    }
    let d = local(buf, distal);
    [0.75 * d.p[0], 0.75 * d.p[1], 0.75 * d.p[2]]
}

// ---------------------------------------------------------------- shapes

/// The signed distance from p (the thing's frame) to its surface. A cone is a frustum along z: radius h[0] at -h[2],
/// h[1] at +h[2] (a capped cone); a capsule's segment runs along z, h[2] each way, of radius h[0].
fn sdf(shape: i32, h: &V3, p: &V3) -> f32 {
    match shape {
        K_SPHERE => len(p) - h[0],
        K_CAPSULE => {
            let z = p[2] - clamp(p[2], -h[2], h[2]);
            sqrt(p[0] * p[0] + p[1] * p[1] + z * z) - h[0]
        }
        K_CYLINDER => {
            let r = hypot(p[0], p[1]) - h[0];
            let z = p[2].abs() - h[2];
            min(max(r, z), 0.0) + hypot(max(r, 0.0), max(z, 0.0))
        }
        K_CONE => {
            let q = hypot(p[0], p[1]);
            let (r1, r2, hh) = (h[0], h[1], h[2]);
            let k2 = [r2 - r1, 2.0 * hh];
            let ca = [q - min(q, if p[2] < 0.0 { r1 } else { r2 }), p[2].abs() - hh];
            let t = clamp(((r2 - q) * k2[0] + (hh - p[2]) * k2[1]) / (k2[0] * k2[0] + k2[1] * k2[1]), 0.0, 1.0);
            let cb = [q - r2 + k2[0] * t, p[2] - hh + k2[1] * t];
            let s = if cb[0] < 0.0 && ca[1] < 0.0 { -1.0 } else { 1.0 };
            s * sqrt(min(ca[0] * ca[0] + ca[1] * ca[1], cb[0] * cb[0] + cb[1] * cb[1]))
        }
        _ => {
            let q = [p[0].abs() - h[0], p[1].abs() - h[1], p[2].abs() - h[2]];
            let o = [max(q[0], 0.0), max(q[1], 0.0), max(q[2], 0.0)];
            len(&o) + min(max(q[0], max(q[1], q[2])), 0.0)
        }
    }
}

/// THE WHOLE OF A ROUND THING, for the fingers to rest on where it is not the part the palm is put to (a bottle's
/// shoulder under the little finger of a hand round its neck): its radius round the part's own axis at `knots` heights
/// along it (ascending, in the part's frame), the solid that profile makes turned round the axis.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GraspLathe {
    pub knots: i32,
    pub z: [f32; 18],
    pub r: [f32; 18],
}
/// The part of a round long thing a hand closes on (grasp_part_of): a cylinder on the thing's axis at `z`, its half
/// length and radius, the ends it may be taken by (as grasp_take's), whether it is a neck or an internal precision stem,
/// and the whole thing round it. A stem has ends 0.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GraspPart {
    pub z: f32,
    pub half: f32,
    pub radius: f32,
    pub ends: i32,
    pub neck: bool,
    pub stem: bool,
    pub lathe: GraspLathe,
}
impl Default for GraspPart {
    fn default() -> Self {
        GraspPart { z: 0.0, half: 0.0, radius: 0.0, ends: 15, neck: false, stem: false, lathe: GraspLathe::default() }
    }
}

/// The signed distance from p (in the part's frame) to a lathe: to its section in (distance from the axis, height), the
/// profile and its mirror across the axis closed by the two end caps (the far side is never nearer than the near one).
fn lathe_sdf(l: &GraspLathe, p: &V3) -> f32 {
    let n = l.knots.clamp(0, 18) as usize;
    let q = hypot(p[0], p[1]);
    let z = p[2];
    let mut v = [[0.0f32; 2]; 36];
    for i in 0..n {
        v[i] = [l.r[i], l.z[i]];
        v[2 * n - 1 - i] = [-l.r[i], l.z[i]];
    }
    let m = 2 * n;
    let mut d = (q - v[0][0]) * (q - v[0][0]) + (z - v[0][1]) * (z - v[0][1]);
    let mut s = 1.0f32;
    let mut j = m - 1;
    for i in 0..m {
        let e = [v[j][0] - v[i][0], v[j][1] - v[i][1]];
        let w = [q - v[i][0], z - v[i][1]];
        let ee = e[0] * e[0] + e[1] * e[1];
        let t = if ee > 1e-12 { clamp((w[0] * e[0] + w[1] * e[1]) / ee, 0.0, 1.0) } else { 0.0 };
        let b = [w[0] - e[0] * t, w[1] - e[1] * t];
        d = min(d, b[0] * b[0] + b[1] * b[1]);
        let c0 = z >= v[i][1];
        let c1 = z < v[j][1];
        let c2 = e[0] * w[1] > e[1] * w[0];
        if (c0 && c1 && c2) || (!c0 && !c1 && !c2) {
            s = -s;
        }
        j = i;
    }
    s * sqrt(d)
}

/// How far the thing reaches from its centre along u (unit, slot frame), turned by r.
fn support(shape: i32, h: &V3, r: &Q, u: &V3) -> f32 {
    let l = unrot(r, u);
    let side = sqrt(max(0.0, 1.0 - l[2] * l[2]));
    match shape {
        K_SPHERE => h[0],
        K_CAPSULE => h[0] + h[2] * l[2].abs(),
        K_CYLINDER => h[0] * side + h[2] * l[2].abs(),
        K_CONE => max(h[0], h[1]) * side + h[2] * l[2].abs(),
        _ => h[0] * l[0].abs() + h[1] * l[1].abs() + h[2] * l[2].abs(),
    }
}

/// A plate too thick for a cover's grip, taken by its side in three fingers.
fn side_pinch(shape: i32, h: &V3) -> bool {
    if shape != K_BOX {
        return false;
    }
    let mut s = *h;
    s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
    s[0] < PLATE * s[1] && s[1] > WRAP_THICK && s[0] >= SIDE_PINCH && s[0] <= SIDE_PINCH_MAX
}

/// std::sort of three indices by their half sizes, as MSVC sorts a range this short: an insertion sort.
fn sort3(h: &V3) -> [usize; 3] {
    let mut o = [0usize, 1, 2];
    for mid in 1..3 {
        let val = o[mid];
        if h[val] < h[o[0]] {
            let mut i = mid;
            while i > 0 {
                o[i] = o[i - 1];
                i -= 1;
            }
            o[0] = val;
        } else {
            let mut hole = mid;
            while h[val] < h[o[hole - 1]] {
                o[hole] = o[hole - 1];
                hole -= 1;
            }
            o[hole] = val;
        }
    }
    o
}

/// The grasp a thing of this shape and size takes, and its long axis (a bar's, a can's: z).
fn classify(shape: i32, h: &V3, part: bool) -> (Kind, usize) {
    if shape == K_SPHERE {
        return (
            if h[0] < PINCH_BALL {
                Kind::Pinch
            } else if h[0] < TRIPOD_MID {
                Kind::Tripod
            } else {
                Kind::Ball
            },
            2,
        );
    }
    if shape == K_CYLINDER || shape == K_CONE || shape == K_CAPSULE {
        let radius = max(h[0], h[1]);
        if !part && radius < TRIPOD_MID && h[2] <= TRIPOD_LONG {
            return (Kind::Tripod, 2);
        }
        return (if radius <= WRAP_RADIUS { Kind::Wrap } else { Kind::Flat }, 2);
    }
    let order = sort3(h);
    if h[order[2]] < PINCH_SIZE {
        return (Kind::Pinch, 2);
    }
    // three fingers: `axis` its thinnest way, the one between the pads and the thumb
    if h[order[2]] <= TRIPOD_LONG && h[order[1]] < TRIPOD_MID {
        return (Kind::Tripod, order[0]);
    }
    if h[order[0]] < PLATE * h[order[1]] && h[order[1]] > WRAP_THICK {
        return (if side_pinch(shape, h) { Kind::Tripod } else { Kind::Edge }, order[0]);
    }
    if h[order[1]] <= WRAP_THICK && h[order[2]] >= 1.5 * h[order[1]] {
        return (Kind::Wrap, order[2]);
    }
    (Kind::Flat, order[2])
}

/// The segments' distance: between p1-q1 and p2-q2.
fn seg_seg(p1: &V3, q1: &V3, p2: &V3, q2: &V3) -> f32 {
    let d1 = sub(q1, p1);
    let d2 = sub(q2, p2);
    let r = sub(p1, p2);
    let a = dot(&d1, &d1);
    let e = dot(&d2, &d2);
    let f = dot(&d2, &r);
    let mut s = 0.0f32;
    let mut t = 0.0f32;
    if a > 1e-10 || e > 1e-10 {
        if a <= 1e-10 {
            t = clamp(f / e, 0.0, 1.0);
        } else {
            let c = dot(&d1, &r);
            if e <= 1e-10 {
                s = clamp(-c / a, 0.0, 1.0);
            } else {
                let b = dot(&d1, &d2);
                let den = a * e - b * b;
                s = if den > 1e-12 { clamp((b * f - c * e) / den, 0.0, 1.0) } else { 0.0 };
                t = (b * s + f) / e;
                if t < 0.0 {
                    t = 0.0;
                    s = clamp(-c / a, 0.0, 1.0);
                } else if t > 1.0 {
                    t = 1.0;
                    s = clamp((b - c) / a, 0.0, 1.0);
                }
            }
        }
    }
    let c1 = mad(p1, &d1, s);
    let c2 = mad(p2, &d2, t);
    len(&sub(&c1, &c2))
}

// ---------------------------------------------------------------- the palm

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Palm {
    /// hand frame: the knuckles' middle and the palm's axes
    knuckle: V3,
    n: V3,
    d: V3,
    t: V3,
    /// the palm's slab: the bones' plane between the wrist and the knuckles
    base: V3,
    half_d: f32,
    half_t: f32,
    /// the palm's skin over its middle
    skin: V3,
    /// the index finger's knuckle, the little finger's
    index: V3,
    pinky: V3,
    /// the index pad bent for a pinch, and the way out of it
    pinch: V3,
    pinch_n: V3,
    /// between the index and middle pads bent so, and the way out of them
    tri: V3,
    tri_n: V3,
    /// the same bent as a claw closes (SIDE_BEND): a thick plate's side
    side: V3,
    side_n: V3,
    /// the weapon slot in the hand's frame
    slot: Pose,
}

/// Each segment a capsule round its bone's axis moved `palm` toward the palm (the way its hinge bends it) with `radius`:
/// the game's first-person hand (a0_000_pwa_fpp__full_r) measured round the rig's bones at its reference -- the bones
/// run near the back of the fingers, so the capsules round the bone let the pads 3-9 mm into what they closed on.
struct Finger {
    slot: [usize; 3],
    max_deg: [f32; 3],
    speed: [f32; 3],
    radius: [f32; 3],
    palm: [f32; 3],
    spread: f32,
}
const FINGERS: [Finger; 4] = [
    Finger { slot: [6, 11, 15], max_deg: [88.0, 105.0, 80.0], speed: [1.0, 1.15, 0.85], radius: [0.0100, 0.0065, 0.0059], palm: [0.0022, 0.0046, 0.0049], spread: 1.0 },
    Finger { slot: [7, 12, 16], max_deg: [88.0, 105.0, 80.0], speed: [1.0, 1.15, 0.85], radius: [0.0110, 0.0077, 0.0062], palm: [0.0030, 0.0056, 0.0049], spread: 0.0 },
    Finger { slot: [8, 13, 17], max_deg: [88.0, 105.0, 80.0], speed: [1.0, 1.15, 0.85], radius: [0.0094, 0.0072, 0.0060], palm: [0.0012, 0.0033, 0.0029], spread: -1.0 },
    Finger { slot: [9, 14, 18], max_deg: [88.0, 105.0, 80.0], speed: [1.0, 1.15, 0.85], radius: [0.0078, 0.0053, 0.0051], palm: [0.0020, 0.0031, 0.0028], spread: -1.8 },
];
const THUMB_SLOTS: [usize; 3] = [0, 5, 10];
/// the thumb's (the metacarpal's is the thenar's: not kept out of things)
const THUMB_RADIUS: [f32; 3] = [0.012, 0.0104, 0.0079];
const THUMB_PALM: [f32; 3] = [0.0, 0.0030, 0.0023];
/// The thumb's end phalanx as the real one: at each share `u` of its length from the joint, the skin off its bone toward
/// the pad, to either side and toward the nail (m). The pad is thick near the joint and thins to the tip.
#[derive(Clone, Copy)]
struct TipSlice {
    u: f32,
    pad: f32,
    side: f32,
    back: f32,
}
const THUMB_TIP: [TipSlice; 7] = [
    TipSlice { u: 0.05, pad: 0.0101, side: 0.0091, back: 0.0051 },
    TipSlice { u: 0.25, pad: 0.0102, side: 0.0088, back: 0.0041 },
    TipSlice { u: 0.45, pad: 0.0087, side: 0.0089, back: 0.0044 },
    TipSlice { u: 0.65, pad: 0.0075, side: 0.0085, back: 0.0048 },
    TipSlice { u: 0.75, pad: 0.0060, side: 0.0082, back: 0.0056 },
    TipSlice { u: 0.85, pad: 0.0040, side: 0.0076, back: 0.0056 },
    TipSlice { u: 0.95, pad: 0.0021, side: 0.0062, back: 0.0061 },
];
/// A slice's skin off the bone in the unit way `toward` (an ellipse round it each side of the pad's way `pad_n`).
fn tip_extent(s: &TipSlice, pad_n: &V3, toward: &V3) -> f32 {
    let c = dot(pad_n, toward);
    let a = if c >= 0.0 { s.pad } else { s.back };
    sqrt(a * a * c * c + s.side * s.side * max(0.0, 1.0 - c * c))
}
/// A segment's axis: the bone's from joint a to joint b moved `off` toward the palm, the way `hinge_local` (in the
/// bone's frame `q`) bends it.
fn seg_axis(a: &V3, b: &V3, q: &Q, hinge_local: &V3, off: f32) -> (V3, V3) {
    let mut ax = sub(b, a);
    unit(&mut ax);
    let hw = rot(q, hinge_local);
    let mut pd = cross(&hw, &ax);
    unit(&mut pd);
    (mad(a, &pd, off), mad(b, &pd, off))
}
const PINCH_BEND: [f32; 3] = [32.0 * K_DEG, 38.0 * K_DEG, 16.0 * K_DEG];
/// A thick plate's side in three fingers, a crab's claw: the index and middle fingers closed already curled (SIDE_CURL)
/// and led by the knuckle from there, met by the plate at its pads so bent (SIDE_BEND).
const SIDE_CURL: [f32; 3] = [0.0, 25.0 * K_DEG, 10.0 * K_DEG];
const SIDE_BEND: [f32; 3] = [28.0 * K_DEG, 31.0 * K_DEG, 15.0 * K_DEG];
/// ...the ring and little finger tucked under it, closed already curled into the fist
const TUCK_CURL: [f32; 3] = [0.0, 60.0 * K_DEG, 40.0 * K_DEG];

/// A finger's chain: its bones, the frame its first hangs from, their locals (rotations from `start`), the tip.
#[derive(Clone, Copy)]
struct ChainBones {
    bones: [i32; 3],
    base: Pose,
    local: [Pose; 3],
    tip: V3,
}
unsafe fn chain_of(buf: *const u8, side: i32, hand: i32, slots: &[usize; 3], start: &[Q; SLOTS]) -> Option<ChainBones> {
    let mut c = ChainBones { bones: [0; 3], base: POSE0, local: [POSE0; 3], tip: [0.0; 3] };
    for j in 0..3 {
        c.bones[j] = VRIK_LadderFingerBone(side, slots[j] as i32);
        if !valid_bone(c.bones[j]) {
            return None;
        }
    }
    c.base = in_frame(buf, side, g_VRBoneParent[c.bones[0] as usize] as i32, hand, Some(start))?;
    c.tip = tip_offset(buf, c.bones[2]);
    for j in 0..3 {
        c.local[j] = local(buf, c.bones[j]);
        c.local[j].q = start[slots[j]];
    }
    Some(c)
}
/// The joints and the tip (hand frame) and the bones' frames, from the three bones' local rotations.
fn joints(c: &ChainBones, rotations: &[Q; 3]) -> ([V3; 4], [Pose; 3]) {
    let mut jts = [[0.0f32; 3]; 4];
    let mut f = [POSE0; 3];
    let mut acc = c.base;
    for j in 0..3 {
        let mut l = c.local[j];
        l.q = rotations[j];
        acc = compose(&acc, &l);
        f[j] = acc;
        jts[j] = acc.p;
    }
    let r = rot(&f[2].q, &c.tip);
    for k in 0..3 {
        jts[3][k] = f[2].p[k] + r[k];
    }
    (jts, f)
}
/// Each joint's flexion hinge, in its bone's frame: the bone axis nearest seg x toward.
fn hinges(jts: &[V3; 4], f: &[Pose; 3], toward: &V3) -> Option<[V3; 3]> {
    let mut h = [[0.0f32; 3]; 3];
    for j in 0..3 {
        let seg = sub(&jts[j + 1], &jts[j]);
        let mut k = cross(&seg, toward);
        if !unit(&mut k) {
            return None;
        }
        h[j] = local_hinge(&f[j].q, &k);
    }
    Some(h)
}

/// The slot in the hand's frame as the rig has it: the frame a held thing's slot is held at (pin_slot), whatever the
/// animation does with it meanwhile, so a seat is the same before the hand holds and while it does.
unsafe fn slot_frame(buf: *const u8, side: i32, hand: i32, slot: i32, reference: &[Q; SLOTS]) -> Option<Pose> {
    let parent = in_frame(buf, side, g_VRBoneParent[slot as usize] as i32, hand, Some(reference))?;
    let mut l = local(buf, slot);
    let mut p = [0.0f32; 3];
    let mut q = [0.0f32; 4];
    if VRIK_ReferenceLocal(slot, p.as_mut_ptr(), q.as_mut_ptr()) {
        l.p = p;
        l.q = q;
        qnorm(&mut l.q);
    }
    Some(compose(&parent, &l))
}

unsafe fn measure_palm(buf: *const u8, side: i32, hand: i32, slot_bone: i32, pm: &mut Palm) -> bool {
    let mut reference = [ID; SLOTS];
    reference_fingers(buf, side, &mut reference, None);
    let index1 = VRIK_LadderFingerBone(side, 6);
    let middle1 = VRIK_LadderFingerBone(side, 7);
    let ring1 = VRIK_LadderFingerBone(side, 8);
    let pinky1 = VRIK_LadderFingerBone(side, 9);
    let middle2 = VRIK_LadderFingerBone(side, 12);
    for b in [index1, middle1, ring1, pinky1, middle2, slot_bone] {
        if !valid_bone(b) {
            return false;
        }
    }
    let Some(i1) = in_frame(buf, side, index1, hand, Some(&reference)) else { return false };
    let Some(m1) = in_frame(buf, side, middle1, hand, Some(&reference)) else { return false };
    let Some(r1) = in_frame(buf, side, ring1, hand, Some(&reference)) else { return false };
    let Some(p1) = in_frame(buf, side, pinky1, hand, Some(&reference)) else { return false };
    let Some(slot) = slot_frame(buf, side, hand, slot_bone, &reference) else { return false };
    pm.slot = slot;
    for k in 0..3 {
        pm.knuckle[k] = 0.25 * (i1.p[k] + m1.p[k] + r1.p[k] + p1.p[k]);
    }
    pm.index = i1.p;
    pm.pinky = p1.p;
    pm.d = m1.p;
    if !unit(&mut pm.d) {
        return false;
    }
    let across = sub(&i1.p, &p1.p);
    let width = len(&across);
    pm.t = mad(&across, &pm.d, -dot(&across, &pm.d));
    if !unit(&mut pm.t) {
        return false;
    }
    pm.n = cross(&pm.d, &pm.t);
    if !unit(&mut pm.n) {
        return false;
    }
    // The side the fingers close to: the middle finger's proximal phalanx turned about its own -Z (the rig's flexion on
    // both hands) takes its next joint there. The hand's handedness says the same (right: -(d x t)); a disagreement is
    // counted, the flexion wins.
    let next = local(buf, middle2);
    let z = [0.0f32, 0.0, -1.0];
    let bend = axis_angle(&z, 20.0 * K_DEG);
    let turned = qmul(&m1.q, &bend);
    let a = rot(&m1.q, &next.p);
    let b = rot(&turned, &next.p);
    let moved = sub(&b, &a);
    let flexion = dot(&moved, &pm.n) >= 0.0;
    if !flexion {
        pm.n = neg(&pm.n);
    }
    CyberpunkVR_DebugGraspNormalFlip[side as usize] = if flexion == (side == 1) { 1 } else { 0 };
    for k in 0..3 {
        pm.base[k] = 0.5 * pm.knuckle[k];
    }
    pm.half_d = 0.5 * dot(&pm.knuckle, &pm.d);
    pm.half_t = 0.5 * width + 0.008;
    pm.skin = mad(&pm.base, &pm.n, PALM_SKIN);
    // The pinch: the index finger bent a little, its pad -- the distal phalanx's middle on the side it bends to. The
    // middle finger's the same way, for three fingers.
    let n = pm.n;
    let pad = |slots: &[usize; 3], bend: &[f32; 3]| -> Option<(V3, V3)> {
        let c = chain_of(buf, side, hand, slots, &reference)?;
        let mut rotations = [reference[slots[0]], reference[slots[1]], reference[slots[2]]];
        let (jts, f) = joints(&c, &rotations);
        let hinge = hinges(&jts, &f, &n)?;
        for j in 0..3 {
            let turn = axis_angle(&hinge[j], bend[j]);
            rotations[j] = qmul(&reference[slots[j]], &turn);
            qnorm(&mut rotations[j]);
        }
        let (jts, f) = joints(&c, &rotations);
        let seg = sub(&jts[3], &jts[2]);
        let axis = rot(&f[2].q, &hinge[2]);
        let mut out = cross(&axis, &seg);
        if !unit(&mut out) {
            return None;
        }
        let mut at = [0.0f32; 3];
        for k in 0..3 {
            at[k] = jts[2][k] + 0.55 * seg[k];
        }
        Some((mad(&at, &out, PINCH_PAD), out))
    };
    let Some((pinch, pinch_n)) = pad(&FINGERS[0].slot, &PINCH_BEND) else { return false };
    pm.pinch = pinch;
    pm.pinch_n = pinch_n;
    let Some((middle, middle_n)) = pad(&FINGERS[1].slot, &PINCH_BEND) else { return false };
    for k in 0..3 {
        pm.tri[k] = 0.5 * (pm.pinch[k] + middle[k]);
        pm.tri_n[k] = pm.pinch_n[k] + middle_n[k];
    }
    if !unit(&mut pm.tri_n) {
        return false;
    }
    let Some((si, si_n)) = pad(&FINGERS[0].slot, &SIDE_BEND) else { return false };
    let Some((sm, sm_n)) = pad(&FINGERS[1].slot, &SIDE_BEND) else { return false };
    for k in 0..3 {
        pm.side[k] = 0.5 * (si[k] + sm[k]);
        pm.side_n[k] = si_n[k] + sm_n[k];
    }
    if !unit(&mut pm.side_n) {
        return false;
    }
    pm.half_d > 0.01
}

// ---------------------------------------------------------------- how it is taken

/// A long thing's axis and half length, in the hand's frame, for a thing at `pose` there.
fn long_axis(shape: i32, h: &V3, axis: usize, pose: &Pose) -> (V3, f32) {
    (rot(&pose.q, &e_axis(axis)), if shape == K_BOX { h[axis] } else { h[2] })
}

/// The grasp the hand takes a thing by, from where the hand is on it: its shape's, but a long thing taken by an end is
/// TOP or CRADLE. `held` is the thing in the hand's frame, `up` the world's up there (None: unknown, so not CRADLE).
/// `ends`: which of its ends may be taken so -- bit 0 its -Z end from above, 1 its +Z end, 2 its -Z end from below, 3
/// +Z. Returns the kind and the long axis; `note` gets the read-out.
fn approach(shape: i32, h: &V3, held: &Pose, pm: &Palm, up: Option<&V3>, note: Option<&mut [f32; 4]>, ends: i32, part: bool) -> (Kind, usize) {
    let (kind, axis) = classify(shape, h, part);
    if kind != Kind::Wrap {
        return (kind, axis);
    }
    let (a, length) = long_axis(shape, h, axis, held);
    let rel = sub(&pm.skin, &held.p);
    let along = dot(&rel, &a);
    let end = if along >= 0.0 { a } else { neg(&a) };
    let end_up = up.map_or(9.0, |u| dot(&end, u));
    let mut out = Kind::Wrap;
    if dot(&pm.n, &a).abs() >= END_ON && along.abs() >= END_NEAR * length && dot(&pm.n, &end) <= 0.0 {
        out = if up.is_some() && end_up < -END_DOWN { Kind::Cradle } else { Kind::Top };
        // that end of the thing's own Z (the axis's sign against it): one it may not be taken by is wrapped instead
        let z = rot(&held.q, &[0.0, 0.0, 1.0]);
        let plus = dot(&end, &z) >= 0.0;
        let bit = (if out == Kind::Cradle { 4 } else { 1 }) << (if plus { 1 } else { 0 });
        if shape == K_CYLINDER && (ends & bit) == 0 {
            out = Kind::Wrap;
        }
    }
    if let Some(note) = note {
        note[0] = dot(&pm.n, &a).abs();
        note[1] = if length > 0.0 { along / length } else { 0.0 };
        note[2] = end_up;
        note[3] = out as i32 as f32;
    }
    (out, axis)
}

/// The grasp of a thing already seated (the solve): its shape's; a long thing standing on the palm along its normal is
/// TOP when its end is off the skin as the seat leaves it, CRADLE when it is on it.
fn seated(shape: i32, h: &V3, th: &Pose, pm: &Palm, part: bool) -> (Kind, usize) {
    let (kind, axis) = classify(shape, h, part);
    if kind != Kind::Wrap {
        return (kind, axis);
    }
    let (a, length) = long_axis(shape, h, axis, th);
    let c = dot(&pm.n, &a).abs();
    if c < END_ON {
        return (Kind::Wrap, axis);
    }
    let rel = sub(&th.p, &pm.skin);
    let gap = dot(&rel, &pm.n) - length * c;
    (if gap > 0.5 * (TOP_GAP + CRADLE_RISE) { Kind::Top } else { Kind::Cradle }, axis)
}

/// The world's up (the model's +Z) in each hand's frame, from the hand as the last pose pass left it (grasp_fingers):
/// the pass's own bones, so it holds in the lobby too, where no whole-skeleton snapshot is taken.
static UP: Mutex<[Option<V3>; 2]> = Mutex::new([None, None]);
fn up_in_hand(side: usize) -> Option<V3> {
    if side > 1 {
        return None;
    }
    UP.lock().unwrap_or_else(|e| e.into_inner())[side]
}
/// A bone's turn in the model's frame: its parents' local turns, the root's first.
unsafe fn model_turn(buf: *const u8, bone: i32) -> Option<Q> {
    let mut chain = [0i32; 64];
    let mut n = 0usize;
    let mut b = bone;
    while b >= 0 {
        if !valid_bone(b) || n >= 64 {
            return None;
        }
        chain[n] = b;
        n += 1;
        b = g_VRBoneParent[b as usize] as i32;
    }
    let mut acc = ID;
    for i in (0..n).rev() {
        let l = local(buf, chain[i]);
        let mut m = qmul(&acc, &l.q);
        qnorm(&mut m);
        acc = m;
    }
    (acc[0] + acc[1] + acc[2] + acc[3]).is_finite().then_some(acc)
}
unsafe fn note_up(buf: *const u8, side: usize, hand: i32) {
    let Some(q) = model_turn(buf, hand) else { return };
    let up = unrot(&q, &[0.0, 0.0, 1.0]);
    UP.lock().unwrap_or_else(|e| e.into_inner())[side] = Some(up);
}

// ---------------------------------------------------------------- the state

/// `force`: the grasp's kind as given (a person's part), not the shape's; `limit` the share of each joint's range the
/// fingers may close (of the kind's own); `spread` the index finger's spread (radians, the others by their share) when
/// given (>= 0), not the kind's.
#[derive(Clone, Copy)]
struct Request {
    shape: i32,
    half: V3,
    obj: Pose,
    ends: i32,
    part: bool,
    lathe: GraspLathe,
    force: i32,
    limit: f32,
    spread: f32,
    /// A BAKED GRASP (grasp_take_baked): the fingers as the fitter closed them on the drawn mesh, as turns off the rig's
    /// reference -- put on, not solved.
    baked: bool,
    baked_kind: i32,
    baked_delta: [Q; SLOTS],
}
const REQUEST0: Request = Request {
    shape: K_BOX,
    half: [0.0; 3],
    obj: POSE0,
    ends: 15,
    part: false,
    lathe: GraspLathe { knots: 0, z: [0.0; 18], r: [0.0; 18] },
    force: -1,
    limit: 1.0,
    spread: -1.0,
    baked: false,
    baked_kind: 0,
    baked_delta: [[0.0; 4]; SLOTS],
};

struct Side {
    active: bool,
    /// the slot held at the rig's reference while the hand holds (VRGraspPin)
    pin: bool,
    rev: u32,
    solved: u32,
    req: Request,
    blend: f32,
    rot: [Q; SLOTS],
    have: [bool; SLOTS],
    palm: bool,
    pm: Palm,
    /// A PERSON'S PART (grasp_person): eased in and out here, not by the caller; re-solved as the part moves in the hand,
    /// the fingers drawn eased from the last solve to the next (`shown`), so they never jump between two.
    person: bool,
    person_on: bool,
    shown_on: bool,
    style: i32,
    eased_at: f64,
    solved_at: f64,
    shown: [Q; SLOTS],
    taken: u32,
    resolved: u32,
}
const PALM0: Palm = Palm {
    knuckle: [0.0; 3],
    n: [0.0; 3],
    d: [0.0; 3],
    t: [0.0; 3],
    base: [0.0; 3],
    half_d: 0.0,
    half_t: 0.0,
    skin: [0.0; 3],
    index: [0.0; 3],
    pinky: [0.0; 3],
    pinch: [0.0; 3],
    pinch_n: [0.0; 3],
    tri: [0.0; 3],
    tri_n: [0.0; 3],
    side: [0.0; 3],
    side_n: [0.0; 3],
    slot: POSE0,
};
const SIDE0: Side = Side {
    active: false,
    pin: true,
    rev: 0,
    solved: !0u32,
    req: REQUEST0,
    blend: 0.0,
    rot: [[0.0; 4]; SLOTS],
    have: [false; SLOTS],
    palm: false,
    pm: PALM0,
    person: false,
    person_on: false,
    shown_on: false,
    style: 0,
    eased_at: 0.0,
    solved_at: 0.0,
    shown: [[0.0; 4]; SLOTS],
    taken: 0,
    resolved: 0,
};
static SIDES: [Mutex<Side>; 2] = [Mutex::new(SIDE0), Mutex::new(SIDE0)];

fn lock(side: usize) -> MutexGuard<'static, Side> {
    SIDES[side].lock().unwrap_or_else(|e| e.into_inner())
}

/// GraspPalmFrame: the palm this hand measured last -- its skin point, its normal and its direction along the fingers,
/// in the hand bone's frame. None before a measure.
pub fn grasp_palm_frame(side: usize) -> Option<([f32; 3], [f32; 3], [f32; 3])> {
    if side > 1 {
        return None;
    }
    let s = lock(side);
    s.palm.then_some((s.pm.skin, s.pm.n, s.pm.d))
}
/// GraspPalm: the palm's skin centre in the hand bone's frame (as the last pass measured it). None when not measured.
pub fn grasp_palm(side: usize) -> Option<[f32; 3]> {
    grasp_palm_frame(side).map(|(skin, _, _)| skin)
}

/// The thing in the hand's frame, ready for distances: the part the palm is put to, or the whole of it round that part
/// when it is known (a lathe: what the fingers rest on).
struct Thing {
    shape: i32,
    half: V3,
    pose: Pose,
    lathe: GraspLathe,
}
impl Thing {
    fn dist(&self, p: &V3) -> f32 {
        let l = to_frame(&self.pose, p);
        if self.lathe.knots >= 2 { lathe_sdf(&self.lathe, &l) } else { sdf(self.shape, &self.half, &l) }
    }
    fn grad(&self, p: &V3) -> V3 {
        let e = 0.0005f32;
        let mut g = [0.0f32; 3];
        for k in 0..3 {
            let mut a = *p;
            let mut b = *p;
            a[k] += e;
            b[k] -= e;
            g[k] = self.dist(&a) - self.dist(&b);
        }
        if !unit(&mut g) {
            g = [0.0, 0.0, 1.0];
        }
        g
    }
    /// The surface point nearest p.
    fn nearest(&self, p: &V3) -> V3 {
        let mut o = *p;
        for _ in 0..4 {
            let g = self.grad(&o);
            let d = self.dist(&o);
            o = mad(&o, &g, -d);
        }
        o
    }
}

fn palm_dist(pm: &Palm, p: &V3) -> f32 {
    let rel = sub(p, &pm.base);
    let q = [dot(&rel, &pm.d).abs() - pm.half_d, dot(&rel, &pm.t).abs() - pm.half_t, dot(&rel, &pm.n).abs() - PALM_SKIN];
    let o = [max(q[0], 0.0), max(q[1], 0.0), max(q[2], 0.0)];
    len(&o) + min(max(q[0], max(q[1], q[2])), 0.0)
}

/// A closed finger: its joints and tip (hand frame), each segment's axis (a, b) and its capsules' radii, the proximal
/// bone's frame.
#[derive(Clone, Copy)]
struct Closed {
    j: [V3; 4],
    a: [V3; 3],
    b: [V3; 3],
    radius: [f32; 3],
    proximal: Pose,
    ok: bool,
}
const CLOSED0: Closed = Closed { j: [[0.0; 3]; 4], a: [[0.0; 3]; 3], b: [[0.0; 3]; 3], radius: [0.0; 3], proximal: POSE0, ok: false };

fn now_s() -> f64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_secs_f64() + 1.0
}

